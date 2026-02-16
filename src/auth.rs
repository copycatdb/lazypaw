//! JWT authentication and role mapping.

use crate::config::AppConfig;
use crate::error::Error;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

/// Authenticate a request using JWT.
///
/// Returns the claims if authentication succeeds, or None for anonymous access.
pub fn authenticate(
    auth_header: Option<&str>,
    config: &AppConfig,
) -> Result<Option<Claims>, Error> {
    let jwt_secret = match &config.jwt_secret {
        Some(s) => s,
        None => {
            // No JWT secret configured — all requests are anonymous
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
            // No auth header — check if anonymous access is allowed
            if config.anon_role.is_some() {
                return Ok(None);
            } else {
                return Err(Error::Unauthorized(
                    "Authentication required".to_string(),
                ));
            }
        }
    };

    let key = DecodingKey::from_secret(jwt_secret.as_bytes());
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    validation.required_spec_claims.clear(); // Don't require any specific claims

    let token_data =
        decode::<Claims>(token, &key, &validation).map_err(|e| {
            Error::Unauthorized(format!("Invalid JWT: {}", e))
        })?;

    Ok(Some(token_data.claims))
}

/// Build SQL to set up the session context for the given claims.
///
/// This sets the SQL Server execution context to the role specified
/// in the JWT and passes claims as session context variables.
pub fn build_session_context_sql(
    claims: &Option<Claims>,
    config: &AppConfig,
) -> Vec<String> {
    let mut stmts = Vec::new();

    let role = claims
        .as_ref()
        .and_then(|c| c.role.as_deref())
        .or(config.anon_role.as_deref());

    // Set execution context
    if let Some(role) = role {
        let safe_role = role.replace('\'', "''");
        stmts.push(format!("EXECUTE AS USER = '{}';", safe_role));
    }

    // Set session context claims
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

/// Build SQL to revert the execution context after a request.
pub fn build_revert_sql() -> &'static str {
    "IF EXISTS (SELECT 1 FROM sys.login_token WHERE usage = 'DENY ONLY') REVERT;"
}
