//! Axum router generation from schema.

use crate::handlers::{self, AppState};
use crate::openapi;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

/// Build the axum router from the schema.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        // OpenAPI spec at root
        .route("/", get(handle_openapi))
        // Swagger UI
        .route("/swagger", get(handle_swagger))
        // RPC endpoint
        .route("/rpc/{procedure}", post(handlers::handle_rpc))
        // Table endpoints: /{table} (default schema) and /{schema}/{table}
        .route(
            "/{*path}",
            get(handle_table_get)
                .post(handle_table_post)
                .patch(handle_table_patch)
                .delete(handle_table_delete),
        )
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Root handler: returns OpenAPI spec.
async fn handle_openapi(State(state): State<AppState>) -> Response {
    let schema = state.schema.read().await;
    let spec = openapi::generate_openapi(&schema, &state.config);
    let json = serde_json::to_string_pretty(&spec).unwrap_or_default();
    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "application/json; charset=utf-8",
        )],
        json,
    )
        .into_response()
}

/// Swagger UI handler.
async fn handle_swagger(State(state): State<AppState>) -> Html<String> {
    Html(openapi::swagger_ui_html(state.config.listen_port))
}

/// Table GET handler — parses wildcard path into path params.
async fn handle_table_get(
    state: State<AppState>,
    axum::extract::Path(path): axum::extract::Path<String>,
    headers: HeaderMap,
    query: axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Response, crate::error::Error> {
    let path_params = parse_wildcard_path(&path);
    handlers::handle_get(
        state,
        axum::extract::Path(path_params),
        headers,
        query,
    )
    .await
}

/// Table POST handler.
async fn handle_table_post(
    state: State<AppState>,
    axum::extract::Path(path): axum::extract::Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, crate::error::Error> {
    let path_params = parse_wildcard_path(&path);
    handlers::handle_post(
        state,
        axum::extract::Path(path_params),
        headers,
        body,
    )
    .await
}

/// Table PATCH handler.
async fn handle_table_patch(
    state: State<AppState>,
    axum::extract::Path(path): axum::extract::Path<String>,
    headers: HeaderMap,
    query: axum::extract::Query<std::collections::HashMap<String, String>>,
    body: axum::body::Bytes,
) -> Result<Response, crate::error::Error> {
    let path_params = parse_wildcard_path(&path);
    handlers::handle_patch(
        state,
        axum::extract::Path(path_params),
        headers,
        query,
        body,
    )
    .await
}

/// Table DELETE handler.
async fn handle_table_delete(
    state: State<AppState>,
    axum::extract::Path(path): axum::extract::Path<String>,
    headers: HeaderMap,
    query: axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Response, crate::error::Error> {
    let path_params = parse_wildcard_path(&path);
    handlers::handle_delete(
        state,
        axum::extract::Path(path_params),
        headers,
        query,
    )
    .await
}

/// Parse a wildcard path into a Vec<(String, String)> for the handlers.
fn parse_wildcard_path(path: &str) -> Vec<(String, String)> {
    // We encode the full path as a single entry for resolve_table_path to parse
    vec![("path".to_string(), path.to_string())]
}
