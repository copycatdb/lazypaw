#![allow(dead_code)]
//! Configuration: CLI args (clap), environment variables, and TOML config file.

use clap::Parser;
use serde::Deserialize;
use std::collections::HashMap;

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

    /// Auth mode: "jwt-secret" or "oidc"
    #[arg(long, env = "LAZYPAW_AUTH_MODE")]
    pub auth_mode: Option<String>,

    /// OIDC issuer URL
    #[arg(long, env = "LAZYPAW_OIDC_ISSUER")]
    pub oidc_issuer: Option<String>,

    /// OIDC expected audience
    #[arg(long, env = "LAZYPAW_OIDC_AUDIENCE")]
    pub oidc_audience: Option<String>,

    /// JWT claim for role lookup (supports dot notation)
    #[arg(long, env = "LAZYPAW_ROLE_CLAIM", default_value = "role")]
    pub role_claim: String,

    /// Comma-separated claims to inject as session context
    #[arg(long, env = "LAZYPAW_CONTEXT_CLAIMS")]
    pub context_claims: Option<String>,

    /// Database auth mode: "password", "managed-identity", "service-principal"
    #[arg(long, env = "LAZYPAW_DB_AUTH", default_value = "password")]
    pub db_auth: String,

    /// Service principal tenant ID
    #[arg(long, env = "LAZYPAW_SP_TENANT_ID")]
    pub sp_tenant_id: Option<String>,

    /// Service principal client ID
    #[arg(long, env = "LAZYPAW_SP_CLIENT_ID")]
    pub sp_client_id: Option<String>,

    /// Service principal client secret
    #[arg(long, env = "LAZYPAW_SP_CLIENT_SECRET")]
    pub sp_client_secret: Option<String>,

    /// Subcommand
    #[command(subcommand)]
    pub subcmd: Option<SubCommand>,

    /// Enable realtime WebSocket endpoint
    #[arg(long, env = "LAZYPAW_REALTIME", default_value = "false")]
    pub realtime: bool,

    /// Realtime poll interval in milliseconds
    #[arg(long, env = "LAZYPAW_REALTIME_POLL_MS", default_value = "200")]
    pub realtime_poll_ms: u64,

    /// Log level (error, warn, info, debug, trace)
    #[arg(long, env = "LAZYPAW_LOG_LEVEL", default_value = "info")]
    pub log_level: String,

    /// Log format (pretty, json)
    #[arg(long, env = "LAZYPAW_LOG_FORMAT", default_value = "pretty")]
    pub log_format: String,

    /// Log slow queries exceeding this threshold (ms)
    #[arg(long, env = "LAZYPAW_LOG_SLOW_QUERIES")]
    pub log_slow_queries: Option<u64>,

    /// Enable OpenTelemetry export
    #[arg(long, env = "LAZYPAW_OTEL_ENABLED", default_value = "false")]
    pub otel_enabled: bool,

    /// OpenTelemetry OTLP endpoint
    #[arg(long, env = "LAZYPAW_OTEL_ENDPOINT", default_value = "http://localhost:4317")]
    pub otel_endpoint: String,

    /// OpenTelemetry service name
    #[arg(long, env = "LAZYPAW_OTEL_SERVICE_NAME", default_value = "lazypaw")]
    pub otel_service_name: String,
}

#[derive(Parser, Debug, Clone)]
pub enum SubCommand {
    /// Generate SQL setup script for roles and impersonation
    Setup {
        /// Comma-separated list of database roles
        #[arg(long)]
        roles: String,

        /// Service account name
        #[arg(long, default_value = "lazypaw_svc")]
        service_account: String,
    },
    /// Generate typed client code from database schema
    Codegen {
        /// Output language: typescript or python
        #[arg(long)]
        lang: String,

        /// Output file path
        #[arg(long)]
        output: String,
    },
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
    pub auth: Option<FileAuthConfig>,
    pub db_config: Option<FileDatabaseConfig>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct FileAuthConfig {
    pub mode: Option<String>,
    pub issuer: Option<String>,
    pub audience: Option<String>,
    pub role_claim: Option<String>,
    pub anon_role: Option<String>,
    pub context_claims: Option<Vec<String>>,
    pub role_map: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct FileDatabaseConfig {
    pub auth: Option<String>,
}

/// Auth mode enumeration.
#[derive(Debug, Clone, PartialEq)]
pub enum AuthMode {
    None,
    JwtSecret,
    Oidc,
}

/// Database authentication mode.
#[derive(Debug, Clone, PartialEq)]
pub enum DbAuthMode {
    Password,
    ManagedIdentity,
    ServicePrincipal,
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
    pub auth_mode: AuthMode,
    pub oidc_issuer: Option<String>,
    pub oidc_audience: Option<String>,
    pub role_claim: String,
    pub context_claims: Vec<String>,
    pub role_map: HashMap<String, String>,
    pub db_auth: DbAuthMode,
    pub sp_tenant_id: Option<String>,
    pub sp_client_id: Option<String>,
    pub sp_client_secret: Option<String>,
    pub realtime: bool,
    pub realtime_poll_ms: u64,
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

        let file_auth = file_config.auth.clone().unwrap_or_default();

        // CLI args override file config
        let schemas = args
            .schemas
            .clone()
            .or(file_config.schemas)
            .map(|s| s.split(',').map(|s| s.trim().to_string()).collect());

        let anon_role = args
            .anon_role
            .clone()
            .or(file_auth.anon_role.clone())
            .or(file_config.anon_role);

        let jwt_secret = args.jwt_secret.clone().or(file_config.jwt_secret);

        // Determine auth mode
        let auth_mode_str = args.auth_mode.clone().or(file_auth.mode.clone());
        let auth_mode = match auth_mode_str.as_deref() {
            Some("oidc") => AuthMode::Oidc,
            Some("jwt-secret") => AuthMode::JwtSecret,
            _ => {
                if jwt_secret.is_some() {
                    AuthMode::JwtSecret
                } else {
                    AuthMode::None
                }
            }
        };

        let oidc_issuer = args.oidc_issuer.clone().or(file_auth.issuer);
        let oidc_audience = args.oidc_audience.clone().or(file_auth.audience);

        let role_claim = if args.role_claim != "role" {
            args.role_claim.clone()
        } else {
            file_auth.role_claim.unwrap_or(args.role_claim.clone())
        };

        let context_claims: Vec<String> = if let Some(ref cc) = args.context_claims {
            cc.split(',').map(|s| s.trim().to_string()).collect()
        } else if let Some(cc) = file_auth.context_claims {
            cc
        } else {
            Vec::new()
        };

        let role_map = file_auth.role_map.unwrap_or_default();

        // DB auth mode
        let db_auth_str = if args.db_auth != "password" {
            args.db_auth.clone()
        } else if let Some(ref db_section) = file_config.db_config {
            db_section
                .auth
                .clone()
                .unwrap_or_else(|| "password".to_string())
        } else {
            "password".to_string()
        };
        let db_auth = match db_auth_str.as_str() {
            "managed-identity" => DbAuthMode::ManagedIdentity,
            "service-principal" => DbAuthMode::ServicePrincipal,
            _ => DbAuthMode::Password,
        };

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
            } else if let Some(file_pw) = file_config.password.filter(|p| !p.is_empty()) {
                file_pw
            } else if let Ok(pw_file) = std::env::var("LAZYPAW_PASSWORD_FILE") {
                match std::fs::read_to_string(&pw_file) {
                    Ok(contents) => contents.trim().to_string(),
                    Err(e) => {
                        tracing::warn!("Could not read LAZYPAW_PASSWORD_FILE {}: {}", pw_file, e);
                        String::new()
                    }
                }
            } else {
                args.password
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
            jwt_secret,
            anon_role,
            pool_size: if args.pool_size != 10 {
                args.pool_size
            } else {
                file_config.pool_size.unwrap_or(args.pool_size)
            },
            trust_cert: args.trust_cert || file_config.trust_cert.unwrap_or(false),
            schemas,
            auth_mode,
            oidc_issuer,
            oidc_audience,
            role_claim,
            context_claims,
            role_map,
            db_auth,
            sp_tenant_id: args.sp_tenant_id,
            sp_client_id: args.sp_client_id,
            sp_client_secret: args.sp_client_secret,
            realtime: args.realtime,
            realtime_poll_ms: args.realtime_poll_ms,
        }
    }
}
