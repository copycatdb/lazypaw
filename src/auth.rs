#![allow(dead_code)]
//! JWT / OIDC authentication, claim mapping, and session SQL generation.

use crate::config::{AppConfig, AuthMode};
use crate::error::Error;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// JWT claims structure.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    /// The role claim (maps to SQL Server user)
    #[serde(default)]
    pub role: Option<String>,

    /// Subject
    #[serde(default)]
    pub sub: Option<String>,

    /// Expiration time (as Unix timestamp)
    #[serde(default)]
    pub exp: Option<u64>,

    /// Issued at
    #[serde(default)]
    pub iat: Option<u64>,

    /// Not before
    #[serde(default)]
    pub nbf: Option<u64>,

    /// All other claims stored as a flat map
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

// ─── OIDC Provider ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct OidcDiscovery {
    jwks_uri: String,
    issuer: String,
}

#[derive(Debug, Deserialize, Clone)]
struct JwksKey {
    kty: String,
    kid: Option<String>,
    #[serde(rename = "use")]
    key_use: Option<String>,
    n: Option<String>,
    e: Option<String>,
    alg: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct JwksResponse {
    keys: Vec<JwksKey>,
}

struct CachedJwks {
    keys: JwksResponse,
    fetched_at: std::time::Instant,
    jwks_uri: String,
}

/// OIDC provider that caches JWKS keys.
pub struct OidcProvider {
    issuer: String,
    cache: RwLock<Option<CachedJwks>>,
    http: reqwest::Client,
}

impl OidcProvider {
    /// Discover OIDC configuration and create provider.
    pub async fn discover(issuer_url: &str) -> Result<Arc<Self>, Error> {
        let http = reqwest::Client::new();
        let discovery_url = format!(
            "{}/.well-known/openid-configuration",
            issuer_url.trim_end_matches('/')
        );

        let disc: OidcDiscovery = http
            .get(&discovery_url)
            .send()
            .await
            .map_err(|e| Error::Internal(format!("OIDC discovery failed: {}", e)))?
            .json()
            .await
            .map_err(|e| Error::Internal(format!("OIDC discovery parse failed: {}", e)))?;

        let provider = Arc::new(Self {
            issuer: disc.issuer,
            cache: RwLock::new(None),
            http,
        });

        // Pre-fetch JWKS
        provider.fetch_jwks(&disc.jwks_uri).await?;

        // Store the jwks_uri in cache
        {
            let mut cache = provider.cache.write().await;
            if let Some(ref mut c) = *cache {
                c.jwks_uri = disc.jwks_uri;
            }
        }

        Ok(provider)
    }

    /// Fetch and cache JWKS keys.
    async fn fetch_jwks(&self, jwks_uri: &str) -> Result<JwksResponse, Error> {
        let keys: JwksResponse = self
            .http
            .get(jwks_uri)
            .send()
            .await
            .map_err(|e| Error::Internal(format!("JWKS fetch failed: {}", e)))?
            .json()
            .await
            .map_err(|e| Error::Internal(format!("JWKS parse failed: {}", e)))?;

        let mut cache = self.cache.write().await;
        *cache = Some(CachedJwks {
            keys: keys.clone(),
            fetched_at: std::time::Instant::now(),
            jwks_uri: jwks_uri.to_string(),
        });

        Ok(keys)
    }

    /// Get cached keys, refreshing if older than 24h.
    async fn get_keys(&self) -> Result<JwksResponse, Error> {
        let cache = self.cache.read().await;
        if let Some(ref c) = *cache {
            if c.fetched_at.elapsed() < std::time::Duration::from_secs(86400) {
                return Ok(c.keys.clone());
            }
            let uri = c.jwks_uri.clone();
            drop(cache);
            return self.fetch_jwks(&uri).await;
        }
        drop(cache);
        Err(Error::Internal("JWKS not initialized".to_string()))
    }

    /// Force refresh keys (on validation failure).
    async fn refresh_keys(&self) -> Result<JwksResponse, Error> {
        let cache = self.cache.read().await;
        let uri = cache
            .as_ref()
            .map(|c| c.jwks_uri.clone())
            .ok_or_else(|| Error::Internal("JWKS not initialized".to_string()))?;
        drop(cache);
        self.fetch_jwks(&uri).await
    }

    /// Validate a JWT token against cached JWKS keys.
    pub async fn validate(&self, token: &str, audience: Option<&str>) -> Result<Claims, Error> {
        let header = decode_header(token)
            .map_err(|e| Error::Unauthorized(format!("Invalid JWT header: {}", e)))?;

        let kid = header.kid.as_deref();
        let alg = header.alg;

        // Only allow RS256/RS384/RS512
        if !matches!(alg, Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512) {
            return Err(Error::Unauthorized(format!(
                "Unsupported algorithm: {:?}",
                alg
            )));
        }

        // Try with cached keys first
        match self.try_validate(token, kid, alg, audience).await {
            Ok(claims) => Ok(claims),
            Err(_) => {
                // Refresh keys and retry once
                self.refresh_keys().await?;
                self.try_validate(token, kid, alg, audience).await
            }
        }
    }

    async fn try_validate(
        &self,
        token: &str,
        kid: Option<&str>,
        alg: Algorithm,
        audience: Option<&str>,
    ) -> Result<Claims, Error> {
        let keys = self.get_keys().await?;

        let jwk = if let Some(kid) = kid {
            keys.keys
                .iter()
                .find(|k| k.kid.as_deref() == Some(kid))
                .ok_or_else(|| {
                    Error::Unauthorized(format!("No matching key found for kid: {}", kid))
                })?
        } else {
            keys.keys
                .first()
                .ok_or_else(|| Error::Unauthorized("No keys in JWKS".to_string()))?
        };

        let n = jwk
            .n
            .as_deref()
            .ok_or_else(|| Error::Unauthorized("Missing RSA modulus in JWKS key".to_string()))?;
        let e = jwk
            .e
            .as_deref()
            .ok_or_else(|| Error::Unauthorized("Missing RSA exponent in JWKS key".to_string()))?;

        let key = DecodingKey::from_rsa_components(n, e)
            .map_err(|e| Error::Unauthorized(format!("Invalid RSA key: {}", e)))?;

        let mut validation = Validation::new(alg);
        validation.set_issuer(&[&self.issuer]);
        if let Some(aud) = audience {
            validation.set_audience(&[aud]);
        } else {
            validation.validate_aud = false;
        }
        validation.validate_exp = true;

        let token_data = decode::<Claims>(token, &key, &validation)
            .map_err(|e| Error::Unauthorized(format!("Invalid JWT: {}", e)))?;

        Ok(token_data.claims)
    }
}

// ─── Authentication ─────────────────────────────────────────

/// Authenticate a request using JWT (HS256) or OIDC (RS256+).
///
/// Returns the claims if authentication succeeds, or None for anonymous access.
pub fn authenticate(
    auth_header: Option<&str>,
    config: &AppConfig,
) -> Result<Option<Claims>, Error> {
    match config.auth_mode {
        AuthMode::None => Ok(None),
        AuthMode::JwtSecret => authenticate_hs256(auth_header, config),
        AuthMode::Oidc => {
            // OIDC validation is async; this sync path is for backward compat.
            // For OIDC, use authenticate_async instead.
            Err(Error::Internal(
                "OIDC auth requires async path; use authenticate_async".to_string(),
            ))
        }
    }
}

/// Async authentication supporting both HS256 and OIDC.
pub async fn authenticate_async(
    auth_header: Option<&str>,
    config: &AppConfig,
    oidc: Option<&OidcProvider>,
) -> Result<Option<Claims>, Error> {
    match config.auth_mode {
        AuthMode::None => {
            if auth_header.is_some() {
                // Token provided but no auth configured — try to decode anyway
                Ok(None)
            } else {
                Ok(None)
            }
        }
        AuthMode::JwtSecret => authenticate_hs256(auth_header, config),
        AuthMode::Oidc => {
            let provider =
                oidc.ok_or_else(|| Error::Internal("OIDC provider not initialized".to_string()))?;

            let token = match auth_header {
                Some(header) => {
                    if let Some(token) = header.strip_prefix("Bearer ") {
                        token.trim()
                    } else {
                        return Err(Error::Unauthorized(
                            "Authorization header must use Bearer scheme".to_string(),
                        ));
                    }
                }
                None => {
                    if config.anon_role.is_some() {
                        return Ok(None);
                    } else {
                        return Err(Error::Unauthorized("Authentication required".to_string()));
                    }
                }
            };

            let claims = provider
                .validate(token, config.oidc_audience.as_deref())
                .await?;
            Ok(Some(claims))
        }
    }
}

/// HS256 JWT authentication (backward compatible).
fn authenticate_hs256(
    auth_header: Option<&str>,
    config: &AppConfig,
) -> Result<Option<Claims>, Error> {
    let jwt_secret = match &config.jwt_secret {
        Some(s) => s,
        None => {
            return Ok(None);
        }
    };

    let token = match auth_header {
        Some(header) => {
            if let Some(token) = header.strip_prefix("Bearer ") {
                token.trim()
            } else {
                return Err(Error::Unauthorized(
                    "Authorization header must use Bearer scheme".to_string(),
                ));
            }
        }
        None => {
            if config.anon_role.is_some() {
                return Ok(None);
            } else {
                return Err(Error::Unauthorized("Authentication required".to_string()));
            }
        }
    };

    let key = DecodingKey::from_secret(jwt_secret.as_bytes());
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    validation.required_spec_claims.clear();

    let token_data = decode::<Claims>(token, &key, &validation)
        .map_err(|e| Error::Unauthorized(format!("Invalid JWT: {}", e)))?;

    Ok(Some(token_data.claims))
}

// ─── Claim Mapping ──────────────────────────────────────────

/// Resolve role from JWT claims using dot-notation path and role_map.
pub fn resolve_role(claims: &Claims, config: &AppConfig) -> Option<String> {
    // Build a combined JSON value of all claims
    let mut all_claims = serde_json::Map::new();
    if let Some(ref role) = claims.role {
        all_claims.insert("role".to_string(), serde_json::Value::String(role.clone()));
    }
    if let Some(ref sub) = claims.sub {
        all_claims.insert("sub".to_string(), serde_json::Value::String(sub.clone()));
    }
    for (k, v) in &claims.extra {
        all_claims.insert(k.clone(), v.clone());
    }

    let root = serde_json::Value::Object(all_claims);

    // Navigate dot notation
    let value = navigate_claim(&root, &config.role_claim)?;

    // If it's an array, find first match in role_map
    match value {
        serde_json::Value::Array(arr) => {
            for item in arr {
                if let serde_json::Value::String(s) = item {
                    if let Some(mapped) = config.role_map.get(s) {
                        return Some(mapped.clone());
                    }
                    // If no role_map or no match, return first string
                    if config.role_map.is_empty() {
                        return Some(s.clone());
                    }
                }
            }
            // No match in array — fall through to anon
            None
        }
        serde_json::Value::String(ref s) => {
            if let Some(mapped) = config.role_map.get(s) {
                Some(mapped.clone())
            } else {
                Some(s.clone())
            }
        }
        other => Some(other.to_string()),
    }
}

/// Navigate a JSON value using dot notation (e.g. "realm_access.roles").
fn navigate_claim<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = value;
    for part in parts {
        match current {
            serde_json::Value::Object(map) => {
                current = map.get(part)?;
            }
            _ => return None,
        }
    }
    Some(current)
}

/// Map a role value to a DB user name using the role_map, falling back to anon_role.
pub fn map_to_db_user(claims: &Option<Claims>, config: &AppConfig) -> Option<String> {
    if let Some(ref c) = claims {
        if let Some(role) = resolve_role(c, config) {
            return Some(role);
        }
    }
    config.anon_role.clone()
}

// ─── Session SQL ────────────────────────────────────────────

/// Build SQL statements for per-request session setup.
///
/// Returns Vec of SQL statements:
///   1. EXECUTE AS USER = '<mapped_db_user>';
///   2. EXEC sp_set_session_context for each context claim
pub fn build_session_sql(claims: &Option<Claims>, config: &AppConfig) -> Vec<String> {
    let mut stmts = Vec::new();

    // Determine DB user
    let db_user = map_to_db_user(claims, config);
    if let Some(ref user) = db_user {
        let safe = user.replace('\'', "''");
        stmts.push(format!("EXECUTE AS USER = '{}';", safe));
    }

    // Set session context claims
    if let Some(ref c) = claims {
        let all_claims = build_claims_map(c);
        for claim_name in &config.context_claims {
            if let Some(val) = all_claims.get(claim_name.as_str()) {
                let val_str = match val {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                let safe_key = claim_name.replace('\'', "''");
                let safe_val = val_str.replace('\'', "''");
                stmts.push(format!(
                    "EXEC sp_set_session_context N'request.jwt.claim.{}', N'{}';",
                    safe_key, safe_val
                ));
            }
        }
    }

    stmts
}

/// Build SQL to set up session context (legacy compat — sets all claims).
pub fn build_session_context_sql(claims: &Option<Claims>, config: &AppConfig) -> Vec<String> {
    // If context_claims is configured, use the new path
    if !config.context_claims.is_empty() {
        return build_session_sql(claims, config);
    }

    // Legacy behavior: set all claims
    let mut stmts = Vec::new();

    let db_user = map_to_db_user(claims, config);
    if let Some(ref user) = db_user {
        let safe = user.replace('\'', "''");
        stmts.push(format!("EXECUTE AS USER = '{}';", safe));
    }

    if let Some(claims) = claims {
        if let Some(ref sub) = claims.sub {
            let safe_sub = sub.replace('\'', "''");
            stmts.push(format!(
                "EXEC sp_set_session_context N'request.jwt.claim.sub', N'{}';",
                safe_sub
            ));
        }
        if let Some(ref role) = claims.role {
            let safe_role = role.replace('\'', "''");
            stmts.push(format!(
                "EXEC sp_set_session_context N'request.jwt.claim.role', N'{}';",
                safe_role
            ));
        }
        for (key, value) in &claims.extra {
            let safe_key = key.replace('\'', "''");
            let val_str = match value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let safe_val = val_str.replace('\'', "''");
            stmts.push(format!(
                "EXEC sp_set_session_context N'request.jwt.claim.{}', N'{}';",
                safe_key, safe_val
            ));
        }
    }

    stmts
}

/// Build REVERT SQL.
pub fn build_revert_sql() -> &'static str {
    "IF EXISTS (SELECT 1 FROM sys.login_token WHERE usage = 'DENY ONLY') REVERT;"
}

/// Build a flat map of all claims.
fn build_claims_map(claims: &Claims) -> HashMap<&str, &serde_json::Value> {
    let mut map = HashMap::new();
    for (k, v) in &claims.extra {
        map.insert(k.as_str(), v);
    }
    map
}
