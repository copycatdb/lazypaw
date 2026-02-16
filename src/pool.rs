//! Connection pool for claw TDS clients.
//!
//! Supports password auth, Azure managed identity, and service principal.

use crate::config::{AppConfig, DbAuthMode};
use crate::error::Error;
use claw::{AuthMethod, Config, TcpClient};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock, Semaphore};

// ─── Token Cache ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: Option<u64>,
    #[serde(default)]
    #[allow(dead_code)]
    expires_on: Option<String>,
}

struct CachedToken {
    token: String,
    expires_at: std::time::Instant,
}

/// Azure AD token provider for managed identity and service principal.
pub struct AadTokenProvider {
    config: AppConfig,
    http: reqwest::Client,
    cache: RwLock<Option<CachedToken>>,
}

impl AadTokenProvider {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
            cache: RwLock::new(None),
        }
    }

    /// Get a valid AAD token, refreshing if needed.
    pub async fn get_token(&self) -> Result<String, Error> {
        // Check cache
        {
            let cache = self.cache.read().await;
            if let Some(ref ct) = *cache {
                // Refresh if within 5 minutes of expiry
                if ct.expires_at > std::time::Instant::now() + std::time::Duration::from_secs(300) {
                    return Ok(ct.token.clone());
                }
            }
        }

        // Fetch new token
        let resp = match self.config.db_auth {
            DbAuthMode::ManagedIdentity => self.fetch_managed_identity_token().await?,
            DbAuthMode::ServicePrincipal => self.fetch_service_principal_token().await?,
            DbAuthMode::Password => {
                return Err(Error::Internal(
                    "Token provider not needed for password auth".to_string(),
                ));
            }
        };

        let expires_in = resp.expires_in.unwrap_or(3600);
        let cached = CachedToken {
            token: resp.access_token.clone(),
            expires_at: std::time::Instant::now() + std::time::Duration::from_secs(expires_in),
        };

        let mut cache = self.cache.write().await;
        *cache = Some(cached);

        Ok(resp.access_token)
    }

    async fn fetch_managed_identity_token(&self) -> Result<TokenResponse, Error> {
        let url = "http://169.254.169.254/metadata/identity/oauth2/token";
        let resp = self
            .http
            .get(url)
            .query(&[
                ("api-version", "2019-08-01"),
                ("resource", "https://database.windows.net/"),
            ])
            .header("Metadata", "true")
            .send()
            .await
            .map_err(|e| Error::Pool(format!("Managed identity token fetch failed: {}", e)))?
            .json::<TokenResponse>()
            .await
            .map_err(|e| Error::Pool(format!("Managed identity token parse failed: {}", e)))?;
        Ok(resp)
    }

    async fn fetch_service_principal_token(&self) -> Result<TokenResponse, Error> {
        let tenant_id = self.config.sp_tenant_id.as_deref().ok_or_else(|| {
            Error::Pool("sp-tenant-id required for service principal auth".to_string())
        })?;
        let client_id = self.config.sp_client_id.as_deref().ok_or_else(|| {
            Error::Pool("sp-client-id required for service principal auth".to_string())
        })?;
        let client_secret = self.config.sp_client_secret.as_deref().ok_or_else(|| {
            Error::Pool("sp-client-secret required for service principal auth".to_string())
        })?;

        let url = format!(
            "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
            tenant_id
        );

        let resp = self
            .http
            .post(&url)
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("scope", "https://database.windows.net/.default"),
            ])
            .send()
            .await
            .map_err(|e| Error::Pool(format!("Service principal token fetch failed: {}", e)))?
            .json::<TokenResponse>()
            .await
            .map_err(|e| Error::Pool(format!("Service principal token parse failed: {}", e)))?;
        Ok(resp)
    }
}

// ─── Pooled Connection ──────────────────────────────────────

/// A pooled connection wrapper.
pub struct PooledConnection {
    client: Option<TcpClient>,
    pool: Arc<Pool>,
}

impl PooledConnection {
    pub fn client(&mut self) -> &mut TcpClient {
        self.client.as_mut().expect("connection taken")
    }
}

impl Drop for PooledConnection {
    fn drop(&mut self) {
        if let Some(client) = self.client.take() {
            let pool = self.pool.clone();
            tokio::spawn(async move {
                pool.return_connection(client).await;
            });
        }
    }
}

// ─── Pool ───────────────────────────────────────────────────

/// Simple async connection pool for TDS connections.
pub struct Pool {
    config: AppConfig,
    connections: Mutex<Vec<TcpClient>>,
    semaphore: Semaphore,
    token_provider: Option<AadTokenProvider>,
}

impl Pool {
    /// Create a new pool with the given configuration.
    pub fn new(config: AppConfig) -> Arc<Self> {
        let size = config.pool_size;
        let token_provider = match config.db_auth {
            DbAuthMode::ManagedIdentity | DbAuthMode::ServicePrincipal => {
                Some(AadTokenProvider::new(config.clone()))
            }
            DbAuthMode::Password => None,
        };
        Arc::new(Self {
            config,
            connections: Mutex::new(Vec::with_capacity(size)),
            semaphore: Semaphore::new(size),
            token_provider,
        })
    }

    /// Get a connection from the pool (or create a new one).
    pub async fn get(self: &Arc<Self>) -> Result<PooledConnection, Error> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|e| Error::Pool(e.to_string()))?;

        let existing = {
            let mut conns = self.connections.lock().await;
            conns.pop()
        };

        let client = match existing {
            Some(c) => c,
            None => self.create_connection().await?,
        };

        std::mem::forget(_permit);

        Ok(PooledConnection {
            client: Some(client),
            pool: Arc::clone(self),
        })
    }

    /// Return a connection to the pool.
    async fn return_connection(&self, client: TcpClient) {
        {
            let mut conns = self.connections.lock().await;
            if conns.len() < self.config.pool_size {
                conns.push(client);
            }
        }
        self.semaphore.add_permits(1);
    }

    /// Create a new TDS connection.
    async fn create_connection(&self) -> Result<TcpClient, Error> {
        let mut config = Config::new();
        config.host(&self.config.server);
        config.port(self.config.port);

        match self.config.db_auth {
            DbAuthMode::Password => {
                config.authentication(AuthMethod::sql_server(
                    &self.config.user,
                    &self.config.password,
                ));
            }
            DbAuthMode::ManagedIdentity | DbAuthMode::ServicePrincipal => {
                let provider = self
                    .token_provider
                    .as_ref()
                    .ok_or_else(|| Error::Pool("Token provider not initialized".to_string()))?;
                let token = provider.get_token().await?;
                config.authentication(AuthMethod::aad_token(&token));
            }
        }

        if self.config.trust_cert {
            config.trust_cert();
        }

        if let Some(ref db) = self.config.database {
            config.database(db);
        }

        let client = claw::connect(config)
            .await
            .map_err(|e| Error::Pool(format!("Connection failed: {}", e)))?;

        Ok(client)
    }
}
