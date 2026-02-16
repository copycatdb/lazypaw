//! lazypaw — CLI entry point.
//!
//! Parses CLI args, loads config, connects to SQL Server,
//! introspects the schema, and launches the axum HTTP server.
//! Handles SIGHUP for live schema reload.

mod auth;
mod config;
mod error;
mod filters;
mod handlers;
mod openapi;
mod pool;
mod query;
mod response;
mod router;
mod schema;
mod select;
mod types;

use clap::Parser;
use config::{AppConfig, Args};
use handlers::AppState;
use pool::Pool;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ── Tracing ──────────────────────────────────────────────
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("lazypaw=info,tower_http=info")),
        )
        .init();

    // ── Config ───────────────────────────────────────────────
    let args = Args::parse();
    let config = AppConfig::from_args(args);

    tracing::info!(
        "😴 lazypaw starting — {}:{} db={:?}",
        config.server,
        config.port,
        config.database
    );

    // ── Connection pool ──────────────────────────────────────
    let pool = Pool::new(config.clone());

    // Verify connectivity
    {
        tracing::info!("Testing database connection...");
        let mut conn = pool.get().await?;
        let client = conn.client();
        let stream = client
            .execute("SELECT 1 AS ok", &[])
            .await
            .map_err(|e| format!("Connection test failed: {}", e))?;
        let _ = stream
            .into_first_result()
            .await
            .map_err(|e| format!("Connection test failed: {}", e))?;
        tracing::info!("Database connection verified ✓");
    }

    // ── Schema introspection ─────────────────────────────────
    tracing::info!("Loading schema...");
    let schema_cache = schema::load_schema(&pool).await?;
    let table_count = schema_cache.tables.len();
    let schema = Arc::new(RwLock::new(schema_cache));
    tracing::info!("Schema loaded: {} tables/views ✓", table_count);

    // ── Build app state & router ─────────────────────────────
    let state = AppState {
        pool: pool.clone(),
        schema: schema.clone(),
        config: config.clone(),
    };
    let app = router::build_router(state);

    // ── SIGHUP handler for schema reload ─────────────────────
    #[cfg(unix)]
    {
        let sighup_pool = pool.clone();
        let sighup_schema = schema.clone();
        tokio::spawn(async move {
            use tokio::signal::unix::{signal, SignalKind};
            let mut hup =
                signal(SignalKind::hangup()).expect("failed to register SIGHUP handler");
            loop {
                hup.recv().await;
                tracing::info!("SIGHUP received — reloading schema...");
                match schema::load_schema(&sighup_pool).await {
                    Ok(new_cache) => {
                        let mut w = sighup_schema.write().await;
                        *w = new_cache;
                        tracing::info!("Schema reloaded ✓");
                    }
                    Err(e) => {
                        tracing::error!("Schema reload failed: {}", e);
                    }
                }
            }
        });
    }

    // ── Start HTTP server ────────────────────────────────────
    let listen_addr = format!("0.0.0.0:{}", config.listen_port);
    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    tracing::info!("Listening on http://{}", listen_addr);
    tracing::info!(
        "OpenAPI spec → http://localhost:{}/",
        config.listen_port
    );
    tracing::info!(
        "Swagger UI   → http://localhost:{}/swagger",
        config.listen_port
    );

    axum::serve(listener, app).await?;

    Ok(())
}
