//! Configuration: CLI args (clap), environment variables, and TOML config file.

use clap::Parser;
use serde::Deserialize;

/// 😴 lazypaw — Instant REST API from your SQL Server database
#[derive(Parser, Debug, Clone)]
#[command(name = "lazypaw", version, about)]
pub struct Args {
    /// SQL Server hostname
    #[arg(long, env = "LAZYPAW_SERVER", default_value = "localhost")]
    pub server: String,

    /// SQL Server port
    #[arg(long, env = "LAZYPAW_DB_PORT", default_value = "1433")]
    pub port: u16,

    /// SQL Server username
    #[arg(long, env = "LAZYPAW_USER", default_value = "sa")]
    pub user: String,

    /// SQL Server password
    #[arg(long, env = "LAZYPAW_PASSWORD", default_value = "")]
    pub password: String,

    /// Database name
    #[arg(long, env = "LAZYPAW_DATABASE")]
    pub database: Option<String>,

    /// HTTP listen port
    #[arg(long, env = "LAZYPAW_LISTEN_PORT", default_value = "3000")]
    pub listen_port: u16,

    /// Default schema (omittable in URLs)
    #[arg(long, env = "LAZYPAW_SCHEMA", default_value = "dbo")]
    pub schema: String,

    /// JWT secret (HS256) for authentication
    #[arg(long, env = "LAZYPAW_JWT_SECRET")]
    pub jwt_secret: Option<String>,

    /// Anonymous role (SQL Server user for unauthenticated requests)
    #[arg(long, env = "LAZYPAW_ANON_ROLE")]
    pub anon_role: Option<String>,

    /// Connection pool size
    #[arg(long, env = "LAZYPAW_POOL_SIZE", default_value = "10")]
    pub pool_size: usize,

    /// Path to TOML config file
    #[arg(long, env = "LAZYPAW_CONFIG")]
    pub config: Option<String>,

    /// Trust server certificate (skip TLS validation)
    #[arg(long, env = "LAZYPAW_TRUST_CERT", default_value = "false")]
    pub trust_cert: bool,

    /// Schemas to expose (comma-separated, default: all)
    #[arg(long, env = "LAZYPAW_SCHEMAS")]
    pub schemas: Option<String>,
}

/// TOML config file structure.
#[derive(Debug, Deserialize, Default)]
pub struct FileConfig {
    pub server: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub database: Option<String>,
    pub listen_port: Option<u16>,
    pub schema: Option<String>,
    pub jwt_secret: Option<String>,
    pub anon_role: Option<String>,
    pub pool_size: Option<usize>,
    pub trust_cert: Option<bool>,
    pub schemas: Option<String>,
}

/// Merged configuration.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub server: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub database: Option<String>,
    pub listen_port: u16,
    pub default_schema: String,
    pub jwt_secret: Option<String>,
    pub anon_role: Option<String>,
    pub pool_size: usize,
    pub trust_cert: bool,
    pub schemas: Option<Vec<String>>,
}

impl AppConfig {
    /// Build config from CLI args, merging in TOML file if provided.
    pub fn from_args(args: Args) -> Self {
        let file_config = if let Some(ref path) = args.config {
            match std::fs::read_to_string(path) {
                Ok(contents) => toml::from_str::<FileConfig>(&contents).unwrap_or_default(),
                Err(e) => {
                    tracing::warn!("Could not read config file {}: {}", path, e);
                    FileConfig::default()
                }
            }
        } else {
            FileConfig::default()
        };

        // CLI args override file config
        let schemas = args
            .schemas
            .or(file_config.schemas)
            .map(|s| s.split(',').map(|s| s.trim().to_string()).collect());

        AppConfig {
            server: if args.server != "localhost" {
                args.server
            } else {
                file_config.server.unwrap_or(args.server)
            },
            port: if args.port != 1433 {
                args.port
            } else {
                file_config.port.unwrap_or(args.port)
            },
            user: if args.user != "sa" {
                args.user
            } else {
                file_config.user.unwrap_or(args.user)
            },
            password: if !args.password.is_empty() {
                args.password
            } else {
                file_config.password.unwrap_or(args.password)
            },
            database: args.database.or(file_config.database),
            listen_port: if args.listen_port != 3000 {
                args.listen_port
            } else {
                file_config.listen_port.unwrap_or(args.listen_port)
            },
            default_schema: if args.schema != "dbo" {
                args.schema
            } else {
                file_config.schema.unwrap_or(args.schema)
            },
            jwt_secret: args.jwt_secret.or(file_config.jwt_secret),
            anon_role: args.anon_role.or(file_config.anon_role),
            pool_size: if args.pool_size != 10 {
                args.pool_size
            } else {
                file_config.pool_size.unwrap_or(args.pool_size)
            },
            trust_cert: args.trust_cert || file_config.trust_cert.unwrap_or(false),
            schemas,
        }
    }
}
