//! Connection pool for claw TDS clients.
//!
//! Since claw's `TcpClient` is not `Send` across await points in a
//! straightforward way, we use a simple async semaphore-based pool
//! that creates connections on demand and recycles them.

use crate::config::AppConfig;
use crate::error::Error;
use claw::{AuthMethod, Config, TcpClient};
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};

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

/// Simple async connection pool for TDS connections.
pub struct Pool {
    config: AppConfig,
    connections: Mutex<Vec<TcpClient>>,
    semaphore: Semaphore,
}

impl Pool {
    /// Create a new pool with the given configuration.
    pub fn new(config: AppConfig) -> Arc<Self> {
        let size = config.pool_size;
        Arc::new(Self {
            config,
            connections: Mutex::new(Vec::with_capacity(size)),
            semaphore: Semaphore::new(size),
        })
    }

    /// Get a connection from the pool (or create a new one).
    pub async fn get(self: &Arc<Self>) -> Result<PooledConnection, Error> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|e| Error::Pool(e.to_string()))?;

        // Try to reuse an existing connection
        let existing = {
            let mut conns = self.connections.lock().await;
            conns.pop()
        };

        let client = match existing {
            Some(c) => c,
            None => self.create_connection().await?,
        };

        // We forget the permit — it'll be released when the PooledConnection is dropped
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
            // else drop the connection
        }
        self.semaphore.add_permits(1);
    }

    /// Create a new TDS connection.
    async fn create_connection(&self) -> Result<TcpClient, Error> {
        let mut config = Config::new();
        config.host(&self.config.server);
        config.port(self.config.port);
        config.authentication(AuthMethod::sql_server(
            &self.config.user,
            &self.config.password,
        ));

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
