//! Request handlers for GET, POST, PATCH, DELETE, and RPC.

use crate::auth;
use crate::config::AppConfig;
use crate::error::Error;
use crate::filters::{self, FilterNode};
use crate::pool::Pool;
use crate::query::{self, escape_ident};
use crate::response::{self, Preferences, ResponseFormat, ReturnMode, TxPreference};
use crate::schema::SchemaCache;
use crate::select::{self, EmbedSelect, SelectNode};
use crate::types;
use axum::body::Bytes;
use axum::extract::{Path, Query as AxumQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use claw::{RowWriter, SqlValue};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub pool: Arc<Pool>,
    pub schema: Arc<RwLock<SchemaCache>>,
    pub config: AppConfig,
}

/// GET handler for table/view queries.
pub async fn handle_get(
    State(state): State<AppState>,
    Path(path_params): Path<Vec<(String, String)>>,
    headers: HeaderMap,
    AxumQuery(query_params): AxumQuery<HashMap<String, String>>,
) -> Result<Response, Error> {
    let (schema_name, table_name) = resolve_table_path(&path_params, &state.config)?;
    let schema_cache = state.schema.read().await;
    let table = schema_cache
        .get_table(&schema_name, &table_name)
        .ok_or_else(|| Error::NotFound(format!("Table not found: {}.{}", schema_name, table_name)))?;

    // Auth
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok());
    let claims = auth::authenticate(auth_header, &state.config)?;

    // Parse parameters
    let format = response::parse_accept(headers.get("accept").and_then(|v| v.to_str().ok()));
    let prefer = response::parse_prefer(headers.get("prefer").and_then(|v| v.to_str().ok()));

    let select_str = query_params.get("select").map(|s| s.as_str()).unwrap_or("*");
    let select_nodes = select::parse_select(select_str)?;

    let limit = query_params.get("limit").and_then(|v| v.parse::<i64>().ok());
    let offset = query_params.get("offset").and_then(|v| v.parse::<i64>().ok());

    // Parse Range header as fallback for limit/offset
    let (range_limit, range_offset) = parse_range_header(&headers);
    let final_limit = limit.or(range_limit);
    let final_offset = offset.or(range_offset);

    let order_str = query_params.get("order").map(|s| s.as_str()).unwrap_or("");
    let order = query::parse_order(order_str)?;

    // Build filters from query params
    let filter_nodes = build_filters_from_params(&query_params, table)?;

    // Build and execute main query
    let built = query::build_select(
        table,
        &select_nodes,
        &filter_nodes,
        &order,
        final_limit,
        final_offset,
        false,
    )?;

    // Get count if requested
    let total_count = if prefer.count {
        let count_query = query::build_select(
            table,
            &select_nodes,
            &filter_nodes,
            &[],
            None,
            None,
            true,
        )?;
        Some(execute_count(&state, &count_query, &claims).await?)
    } else {
        None
    };

    // Execute query using Arrow path or standard path based on Accept header
    match format {
        ResponseFormat::ArrowIpcStream | ResponseFormat::ArrowJson => {
            let batch = execute_arrow_query(&state, &built, &claims).await?;
            match format {
                ResponseFormat::ArrowIpcStream => {
                    let bytes = response::record_batch_to_ipc(&batch)?;
                    let range = build_content_range(
                        final_offset.unwrap_or(0),
                        batch.num_rows() as i64,
                        total_count,
                    );
                    Ok(response::build_response(
                        bytes,
                        "application/vnd.apache.arrow.stream",
                        StatusCode::OK,
                        Some(range),
                        None,
                    ))
                }
                ResponseFormat::ArrowJson => {
                    let json = response::record_batch_to_arrow_json(&batch)?;
                    let range = build_content_range(
                        final_offset.unwrap_or(0),
                        batch.num_rows() as i64,
                        total_count,
                    );
                    Ok(response::build_response(
                        json.into_bytes(),
                        "application/vnd.apache.arrow+json",
                        StatusCode::OK,
                        Some(range),
                        None,
                    ))
                }
                _ => unreachable!(),
            }
        }
        _ => {
            let mut rows = execute_query_to_json(&state, &built, &claims).await?;

            // Handle embeddings
            let embeds = select::select_embeds(&select_nodes);
            if !embeds.is_empty() {
                handle_embeds(
                    &state,
                    &schema_cache,
                    &schema_name,
                    &table_name,
                    &embeds,
                    &mut rows,
                    &query_params,
                    &claims,
                )
                .await?;
            }

            let row_count = rows.len() as i64;
            let range = build_content_range(
                final_offset.unwrap_or(0),
                row_count,
                total_count,
            );

            match format {
                ResponseFormat::SingleObjectJson => {
                    if rows.len() != 1 {
                        return Err(Error::SingleObjectExpected(rows.len()));
                    }
                    let json = serde_json::to_string(&rows[0]).unwrap_or_default();
                    Ok(response::build_response(
                        json.into_bytes(),
                        "application/vnd.pgrst.object+json; charset=utf-8",
                        StatusCode::OK,
                        Some(range),
                        None,
                    ))
                }
                ResponseFormat::Csv => {
                    let columns: Vec<String> = if rows.is_empty() {
                        table.columns.iter().map(|c| c.name.clone()).collect()
                    } else {
                        rows[0].keys().cloned().collect()
                    };
                    let csv_str = response::rows_to_csv(&rows, &columns)?;
                    Ok(response::build_response(
                        csv_str.into_bytes(),
                        "text/csv; charset=utf-8",
                        StatusCode::OK,
                        Some(range),
                        None,
                    ))
                }
                _ => {
                    let json = response::rows_to_json(&rows);
                    Ok(response::build_response(
                        json.into_bytes(),
                        "application/json; charset=utf-8",
                        StatusCode::OK,
                        Some(range),
                        None,
                    ))
                }
            }
        }
    }
}

/// POST handler for inserts.
pub async fn handle_post(
    State(state): State<AppState>,
    Path(path_params): Path<Vec<(String, String)>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, Error> {
    let (schema_name, table_name) = resolve_table_path(&path_params, &state.config)?;
    let schema_cache = state.schema.read().await;
    let table = schema_cache
        .get_table(&schema_name, &table_name)
        .ok_or_else(|| Error::NotFound(format!("Table not found: {}.{}", schema_name, table_name)))?
        .clone();
    drop(schema_cache);

    let auth_header = headers.get("authorization").and_then(|v| v.to_str().ok());
    let claims = auth::authenticate(auth_header, &state.config)?;
    let prefer = response::parse_prefer(headers.get("prefer").and_then(|v| v.to_str().ok()));
    let format = response::parse_accept(headers.get("accept").and_then(|v| v.to_str().ok()));

    let body_str = String::from_utf8(body.to_vec())
        .map_err(|_| Error::BadRequest("Invalid UTF-8 body".to_string()))?;
    let json: JsonValue =
        serde_json::from_str(&body_str).map_err(|e| Error::BadRequest(format!("Invalid JSON: {}", e)))?;

    let is_upsert = prefer
        .resolution
        .as_deref()
        == Some("merge-duplicates");

    // Normalize to array of objects
    let objects: Vec<&serde_json::Map<String, JsonValue>> = match &json {
        JsonValue::Array(arr) => arr
            .iter()
            .map(|v| {
                v.as_object()
                    .ok_or_else(|| Error::BadRequest("Array must contain objects".to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?,
        JsonValue::Object(obj) => vec![obj],
        _ => return Err(Error::BadRequest("Body must be object or array".to_string())),
    };

    if objects.is_empty() {
        return Err(Error::BadRequest("Empty body".to_string()));
    }

    // Get columns from the first object
    let columns: Vec<String> = objects[0].keys().cloned().collect();

    // Build SQL
    let built = if is_upsert {
        query::build_upsert(&table, &columns, objects.len())?
    } else {
        query::build_insert(&table, &columns, objects.len())?
    };

    // Collect all parameter values
    let mut param_values: Vec<String> = Vec::new();
    for obj in &objects {
        for col in &columns {
            let val = obj.get(col).unwrap_or(&JsonValue::Null);
            param_values.push(json_value_to_sql_string(val));
        }
    }

    // Execute
    let rows = execute_dml_query(&state, &built.sql, &param_values, &claims, &prefer).await?;

    build_mutation_response(rows, &prefer, &format, StatusCode::CREATED)
}

/// PATCH handler for updates.
pub async fn handle_patch(
    State(state): State<AppState>,
    Path(path_params): Path<Vec<(String, String)>>,
    headers: HeaderMap,
    AxumQuery(query_params): AxumQuery<HashMap<String, String>>,
    body: Bytes,
) -> Result<Response, Error> {
    let (schema_name, table_name) = resolve_table_path(&path_params, &state.config)?;
    let schema_cache = state.schema.read().await;
    let table = schema_cache
        .get_table(&schema_name, &table_name)
        .ok_or_else(|| Error::NotFound(format!("Table not found: {}.{}", schema_name, table_name)))?
        .clone();
    drop(schema_cache);

    let auth_header = headers.get("authorization").and_then(|v| v.to_str().ok());
    let claims = auth::authenticate(auth_header, &state.config)?;
    let prefer = response::parse_prefer(headers.get("prefer").and_then(|v| v.to_str().ok()));
    let format = response::parse_accept(headers.get("accept").and_then(|v| v.to_str().ok()));

    let body_str = String::from_utf8(body.to_vec())
        .map_err(|_| Error::BadRequest("Invalid UTF-8 body".to_string()))?;
    let obj: serde_json::Map<String, JsonValue> = serde_json::from_str(&body_str)
        .map_err(|e| Error::BadRequest(format!("Invalid JSON: {}", e)))?;

    let columns: Vec<String> = obj.keys().cloned().collect();
    let filter_nodes = build_filters_from_params(&query_params, &table)?;

    let built = query::build_update(&table, &columns, &filter_nodes)?;

    // Collect SET values + WHERE params
    let mut param_values: Vec<String> = columns
        .iter()
        .map(|col| {
            let val = obj.get(col).unwrap_or(&JsonValue::Null);
            json_value_to_sql_string(val)
        })
        .collect();
    param_values.extend(built.params.clone());

    let rows = execute_dml_query(&state, &built.sql, &param_values, &claims, &prefer).await?;

    build_mutation_response(rows, &prefer, &format, StatusCode::OK)
}

/// DELETE handler.
pub async fn handle_delete(
    State(state): State<AppState>,
    Path(path_params): Path<Vec<(String, String)>>,
    headers: HeaderMap,
    AxumQuery(query_params): AxumQuery<HashMap<String, String>>,
) -> Result<Response, Error> {
    let (schema_name, table_name) = resolve_table_path(&path_params, &state.config)?;
    let schema_cache = state.schema.read().await;
    let table = schema_cache
        .get_table(&schema_name, &table_name)
        .ok_or_else(|| Error::NotFound(format!("Table not found: {}.{}", schema_name, table_name)))?
        .clone();
    drop(schema_cache);

    let auth_header = headers.get("authorization").and_then(|v| v.to_str().ok());
    let claims = auth::authenticate(auth_header, &state.config)?;
    let prefer = response::parse_prefer(headers.get("prefer").and_then(|v| v.to_str().ok()));
    let format = response::parse_accept(headers.get("accept").and_then(|v| v.to_str().ok()));

    let filter_nodes = build_filters_from_params(&query_params, &table)?;

    let built = query::build_delete(&table, &filter_nodes)?;

    let rows =
        execute_dml_query(&state, &built.sql, &built.params, &claims, &prefer).await?;

    build_mutation_response(rows, &prefer, &format, StatusCode::OK)
}

/// POST /rpc/<procedure> handler.
pub async fn handle_rpc(
    State(state): State<AppState>,
    Path(proc_name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, Error> {
    let auth_header = headers.get("authorization").and_then(|v| v.to_str().ok());
    let claims = auth::authenticate(auth_header, &state.config)?;
    let format = response::parse_accept(headers.get("accept").and_then(|v| v.to_str().ok()));

    let body_str = String::from_utf8(body.to_vec())
        .map_err(|_| Error::BadRequest("Invalid UTF-8 body".to_string()))?;

    let params: serde_json::Map<String, JsonValue> = if body_str.is_empty() {
        serde_json::Map::new()
    } else {
        serde_json::from_str(&body_str)
            .map_err(|e| Error::BadRequest(format!("Invalid JSON: {}", e)))?
    };

    // Build EXEC statement
    let safe_proc = proc_name.replace('\'', "''").replace(']', "]]");
    let mut sql_parts = Vec::new();
    let mut param_values: Vec<String> = Vec::new();

    for (i, (key, val)) in params.iter().enumerate() {
        let safe_key = key.replace(']', "]]");
        sql_parts.push(format!("@{} = @P{}", safe_key, i + 1));
        param_values.push(json_value_to_sql_string(val));
    }

    let sql = if sql_parts.is_empty() {
        format!("EXEC [{}]", safe_proc)
    } else {
        format!("EXEC [{}] {}", safe_proc, sql_parts.join(", "))
    };

    // Build context SQL
    let ctx_stmts = auth::build_session_context_sql(&claims, &state.config);
    let full_sql = if ctx_stmts.is_empty() {
        format!("SET NOCOUNT ON;\n{}", sql)
    } else {
        format!("SET NOCOUNT ON;\n{}\n{}", ctx_stmts.join("\n"), sql)
    };

    let mut conn = state.pool.get().await?;
    let client = conn.client();

    let mut query = claw::Query::new(full_sql);
    for val in &param_values {
        query.bind(val.as_str());
    }

    let stream = query
        .query(client)
        .await
        .map_err(|e| Error::Sql(e.to_string()))?;

    let rows = stream
        .into_first_result()
        .await
        .map_err(|e| Error::Sql(e.to_string()))?;

    let json_rows: Vec<serde_json::Map<String, JsonValue>> =
        rows.iter().map(types::row_to_json).collect();

    match format {
        ResponseFormat::SingleObjectJson => {
            if json_rows.len() != 1 {
                return Err(Error::SingleObjectExpected(json_rows.len()));
            }
            let json = serde_json::to_string(&json_rows[0]).unwrap_or_default();
            Ok(response::build_response(
                json.into_bytes(),
                "application/vnd.pgrst.object+json; charset=utf-8",
                StatusCode::OK,
                None,
                None,
            ))
        }
        _ => {
            let json = response::rows_to_json(&json_rows);
            Ok(response::build_response(
                json.into_bytes(),
                "application/json; charset=utf-8",
                StatusCode::OK,
                None,
                None,
            ))
        }
    }
}

// ──────────────────── Helper functions ────────────────────

/// Resolve schema and table name from path.
fn resolve_table_path(
    path_params: &[(String, String)],
    config: &AppConfig,
) -> Result<(String, String), Error> {
    // Path can be /<schema>/<table> or /<table> (uses default schema)
    match path_params.len() {
        1 => {
            // Single segment: just table name, use default schema
            let key = &path_params[0].0;
            let val = &path_params[0].1;
            // axum wildcard gives us one entry
            let full = if !val.is_empty() {
                format!("{}/{}", key, val)
            } else {
                key.clone()
            };
            let parts: Vec<&str> = full.split('/').filter(|s| !s.is_empty()).collect();
            match parts.len() {
                1 => Ok((config.default_schema.clone(), parts[0].to_string())),
                2 => Ok((parts[0].to_string(), parts[1].to_string())),
                _ => Err(Error::BadRequest(format!("Invalid path: {}", full))),
            }
        }
        _ => Err(Error::BadRequest("Invalid path".to_string())),
    }
}

/// Build filter nodes from query parameters.
fn build_filters_from_params(
    query_params: &HashMap<String, String>,
    table: &crate::schema::TableInfo,
) -> Result<Vec<FilterNode>, Error> {
    let reserved = [
        "select", "order", "limit", "offset", "and", "or",
    ];

    let mut filter_nodes: Vec<FilterNode> = Vec::new();

    for (key, value) in query_params {
        if reserved.contains(&key.as_str()) {
            continue;
        }

        // Handle "or" and "and" groups
        if key == "or" {
            let group_filters = filters::parse_logic_group(value)?;
            let nodes: Vec<FilterNode> = group_filters
                .into_iter()
                .map(FilterNode::Condition)
                .collect();
            filter_nodes.push(FilterNode::Or(nodes));
            continue;
        }
        if key == "and" {
            let group_filters = filters::parse_logic_group(value)?;
            let nodes: Vec<FilterNode> = group_filters
                .into_iter()
                .map(FilterNode::Condition)
                .collect();
            filter_nodes.push(FilterNode::And(nodes));
            continue;
        }

        // Handle embed filters (e.g., orders.status=eq.active)
        if key.contains('.') {
            // This is an embed filter — skip it for main query,
            // it'll be handled in the embed query
            continue;
        }

        // Check if this is a valid column
        if table.column(key).is_some() {
            let filter = filters::parse_filter(key, value)?;
            filter_nodes.push(FilterNode::Condition(filter));
        }
    }

    Ok(filter_nodes)
}

/// Execute a query and return results as JSON maps.
async fn execute_query_to_json(
    state: &AppState,
    built: &query::BuiltQuery,
    claims: &Option<auth::Claims>,
) -> Result<Vec<serde_json::Map<String, JsonValue>>, Error> {
    let ctx_stmts = auth::build_session_context_sql(claims, &state.config);
    let full_sql = if ctx_stmts.is_empty() {
        format!("SET NOCOUNT ON;\n{}", built.sql)
    } else {
        format!("SET NOCOUNT ON;\n{}\n{}", ctx_stmts.join("\n"), built.sql)
    };

    let mut conn = state.pool.get().await?;
    let client = conn.client();

    let mut query = claw::Query::new(full_sql);
    for val in &built.params {
        query.bind(val.as_str());
    }

    let stream = query
        .query(client)
        .await
        .map_err(|e| Error::Sql(e.to_string()))?;

    let rows = stream
        .into_first_result()
        .await
        .map_err(|e| Error::Sql(e.to_string()))?;

    Ok(rows.iter().map(types::row_to_json).collect())
}

/// Execute a query and return an Arrow RecordBatch.
async fn execute_arrow_query(
    state: &AppState,
    built: &query::BuiltQuery,
    claims: &Option<auth::Claims>,
) -> Result<arrow::record_batch::RecordBatch, Error> {
    let ctx_stmts = auth::build_session_context_sql(claims, &state.config);
    let full_sql = if ctx_stmts.is_empty() {
        format!("SET NOCOUNT ON;\n{}", built.sql)
    } else {
        format!("SET NOCOUNT ON;\n{}\n{}", ctx_stmts.join("\n"), built.sql)
    };

    // For Arrow queries we currently can't use parameterized queries
    // (query_arrow takes raw SQL), so we need to inline params safely.
    // For now, fall back to the parameterized Query + ArrowRowWriter path.
    let mut conn = state.pool.get().await?;
    let client = conn.client();

    let mut writer = claw::ArrowRowWriter::new();

    // Build the full query with params inlined using sp_executesql style
    if built.params.is_empty() {
        client
            .batch_into(&full_sql, &mut writer)
            .await
            .map_err(|e| Error::Sql(e.to_string()))?;
    } else {
        // Use Query to bind params, but we need to use the batch_into approach.
        // Since batch_into doesn't support params, we'll execute via the standard path
        // and convert to Arrow.
        let mut query = claw::Query::new(full_sql);
        for val in &built.params {
            query.bind(val.as_str());
        }

        let stream = query
            .query(client)
            .await
            .map_err(|e| Error::Sql(e.to_string()))?;

        let rows = stream
            .into_first_result()
            .await
            .map_err(|e| Error::Sql(e.to_string()))?;

        // Build RecordBatch from rows
        return rows_to_record_batch(&rows);
    }

    writer
        .finish()
        .map_err(|e| Error::Internal(e.to_string()))
}

/// Convert Vec<Row> to a RecordBatch.
fn rows_to_record_batch(
    rows: &[claw::Row],
) -> Result<arrow::record_batch::RecordBatch, Error> {
    if rows.is_empty() {
        // Return empty batch with no schema
        let schema = std::sync::Arc::new(arrow::datatypes::Schema::empty());
        return Ok(arrow::record_batch::RecordBatch::new_empty(schema));
    }

    // Use ArrowRowWriter by feeding it the metadata and values
    let mut writer = claw::ArrowRowWriter::new();
    let columns = rows[0].columns();
    writer.on_metadata(columns);

    for row in rows {
        for (i, (_col, val)) in row.cells().enumerate() {
            write_sql_value_to_arrow(&mut writer, i, val);
        }
        writer.on_row_done();
    }

    writer
        .finish()
        .map_err(|e| Error::Internal(e.to_string()))
}

/// Write a SqlValue into an ArrowRowWriter at the given column.
fn write_sql_value_to_arrow(writer: &mut claw::ArrowRowWriter, col: usize, val: &SqlValue<'_>) {
    use claw::RowWriter;

    match val {
        SqlValue::U8(Some(v)) => writer.write_u8(col, *v),
        SqlValue::U8(None) => writer.write_null(col),
        SqlValue::I16(Some(v)) => writer.write_i16(col, *v),
        SqlValue::I16(None) => writer.write_null(col),
        SqlValue::I32(Some(v)) => writer.write_i32(col, *v),
        SqlValue::I32(None) => writer.write_null(col),
        SqlValue::I64(Some(v)) => writer.write_i64(col, *v),
        SqlValue::I64(None) => writer.write_null(col),
        SqlValue::F32(Some(v)) => writer.write_f32(col, *v),
        SqlValue::F32(None) => writer.write_null(col),
        SqlValue::F64(Some(v)) => writer.write_f64(col, *v),
        SqlValue::F64(None) => writer.write_null(col),
        SqlValue::Bit(Some(v)) => writer.write_bool(col, *v),
        SqlValue::Bit(None) => writer.write_null(col),
        SqlValue::String(Some(v)) => writer.write_str(col, v),
        SqlValue::String(None) => writer.write_null(col),
        SqlValue::Guid(Some(v)) => {
            writer.write_guid(col, v.as_bytes());
        }
        SqlValue::Guid(None) => writer.write_null(col),
        SqlValue::Binary(Some(v)) => writer.write_bytes(col, v),
        SqlValue::Binary(None) => writer.write_null(col),
        SqlValue::Numeric(Some(v)) => writer.write_decimal(col, v.value(), v.precision(), v.scale()),
        SqlValue::Numeric(None) => writer.write_null(col),
        SqlValue::DateTime(_) | SqlValue::SmallDateTime(_) | SqlValue::DateTime2(_) => {
            // For datetime types, convert to string and write as str
            let json = types::sql_value_to_json(val);
            if let serde_json::Value::String(s) = json {
                writer.write_str(col, &s);
            } else {
                writer.write_null(col);
            }
        }
        SqlValue::Date(Some(d)) => {
            writer.write_date(col, d.days() as i32);
        }
        SqlValue::Date(None) => writer.write_null(col),
        SqlValue::Time(Some(t)) => {
            let nanos = t.increments() as i64 * 10i64.pow(9u32.saturating_sub(t.scale() as u32));
            writer.write_time(col, nanos);
        }
        SqlValue::Time(None) => writer.write_null(col),
        SqlValue::DateTimeOffset(_) => {
            let json = types::sql_value_to_json(val);
            if let serde_json::Value::String(s) = json {
                writer.write_str(col, &s);
            } else {
                writer.write_null(col);
            }
        }
        SqlValue::Xml(Some(v)) => writer.write_str(col, &format!("{}", v)),
        SqlValue::Xml(None) => writer.write_null(col),
    }
}

/// Execute a count query.
async fn execute_count(
    state: &AppState,
    built: &query::BuiltQuery,
    claims: &Option<auth::Claims>,
) -> Result<i64, Error> {
    let rows = execute_query_to_json(state, built, claims).await?;
    if let Some(first) = rows.first() {
        if let Some(count) = first.get("count") {
            return count
                .as_i64()
                .ok_or_else(|| Error::Internal("Invalid count".to_string()));
        }
    }
    Ok(0)
}

/// Execute a DML query (INSERT/UPDATE/DELETE) with OUTPUT.
async fn execute_dml_query(
    state: &AppState,
    sql: &str,
    params: &[String],
    claims: &Option<auth::Claims>,
    prefer: &Preferences,
) -> Result<Vec<serde_json::Map<String, JsonValue>>, Error> {
    let ctx_stmts = auth::build_session_context_sql(claims, &state.config);

    let tx_begin = "BEGIN TRANSACTION;";
    let tx_end = if prefer.tx == TxPreference::Rollback {
        "ROLLBACK TRANSACTION;"
    } else {
        "COMMIT TRANSACTION;"
    };

    let full_sql = if ctx_stmts.is_empty() {
        format!("SET NOCOUNT ON;\n{}\n{}\n{}", tx_begin, sql, tx_end)
    } else {
        format!(
            "SET NOCOUNT ON;\n{}\n{}\n{}\n{}",
            ctx_stmts.join("\n"),
            tx_begin,
            sql,
            tx_end
        )
    };

    let mut conn = state.pool.get().await?;
    let client = conn.client();

    let mut query = claw::Query::new(full_sql);
    for val in params {
        query.bind(val.as_str());
    }

    let stream = query
        .query(client)
        .await
        .map_err(|e| Error::Sql(e.to_string()))?;

    let rows = stream
        .into_first_result()
        .await
        .map_err(|e| Error::Sql(e.to_string()))?;

    Ok(rows.iter().map(types::row_to_json).collect())
}

/// Build a mutation response based on Prefer header.
fn build_mutation_response(
    rows: Vec<serde_json::Map<String, JsonValue>>,
    prefer: &Preferences,
    format: &ResponseFormat,
    success_status: StatusCode,
) -> Result<Response, Error> {
    match prefer.return_mode {
        ReturnMode::Minimal => Ok(response::build_response(
            Vec::new(),
            "application/json",
            StatusCode::NO_CONTENT,
            None,
            None,
        )),
        ReturnMode::HeadersOnly => {
            let range = format!("*/*/{}", rows.len());
            Ok(response::build_response(
                Vec::new(),
                "application/json",
                success_status,
                Some(range),
                None,
            ))
        }
        ReturnMode::Representation => {
            match format {
                ResponseFormat::SingleObjectJson => {
                    if rows.len() != 1 {
                        return Err(Error::SingleObjectExpected(rows.len()));
                    }
                    let json = serde_json::to_string(&rows[0]).unwrap_or_default();
                    Ok(response::build_response(
                        json.into_bytes(),
                        "application/vnd.pgrst.object+json; charset=utf-8",
                        success_status,
                        None,
                        None,
                    ))
                }
                _ => {
                    let json = response::rows_to_json(&rows);
                    Ok(response::build_response(
                        json.into_bytes(),
                        "application/json; charset=utf-8",
                        success_status,
                        None,
                        None,
                    ))
                }
            }
        }
    }
}

/// Handle embedding of related tables.
async fn handle_embeds(
    state: &AppState,
    schema_cache: &SchemaCache,
    schema_name: &str,
    table_name: &str,
    embeds: &[&EmbedSelect],
    rows: &mut Vec<serde_json::Map<String, JsonValue>>,
    _query_params: &HashMap<String, String>,
    claims: &Option<auth::Claims>,
) -> Result<(), Error> {
    for embed in embeds {
        let embed_info = schema_cache
            .find_embed(schema_name, table_name, &embed.name, embed.fk_hint.as_deref())
            .ok_or_else(|| {
                Error::BadRequest(format!(
                    "No relationship found for embed: {}",
                    embed.name
                ))
            })?;

        let target_table = schema_cache
            .get_table(&embed_info.target_schema, &embed_info.target_table)
            .ok_or_else(|| {
                Error::NotFound(format!(
                    "Embedded table not found: {}.{}",
                    embed_info.target_schema, embed_info.target_table
                ))
            })?;

        // Collect source values for the join column
        let source_values: Vec<String> = rows
            .iter()
            .filter_map(|row| {
                row.get(&embed_info.source_column).and_then(|v| match v {
                    JsonValue::Null => None,
                    JsonValue::String(s) => Some(s.clone()),
                    other => Some(other.to_string()),
                })
            })
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        if source_values.is_empty() {
            // No values to join on — set all embeds to empty array
            for row in rows.iter_mut() {
                row.insert(embed.name.clone(), JsonValue::Array(Vec::new()));
            }
            continue;
        }

        // Build embed column list
        let embed_columns = build_embed_column_list(target_table, &embed.columns);

        // Build IN clause for batch fetch
        let placeholders: Vec<String> = source_values
            .iter()
            .enumerate()
            .map(|(i, _)| format!("@P{}", i + 1))
            .collect();

        let embed_sql = format!(
            "SET NOCOUNT ON;\nSELECT {} FROM {} WHERE [{}] IN ({})",
            embed_columns,
            target_table.full_name(),
            escape_ident(&embed_info.target_column),
            placeholders.join(", ")
        );

        // Apply embed filters
        let _embed_filter_prefix = format!("{}.", embed.name);

        let ctx_stmts = auth::build_session_context_sql(claims, &state.config);
        let full_sql = if ctx_stmts.is_empty() {
            embed_sql
        } else {
            format!("{}\n{}", ctx_stmts.join("\n"), embed_sql)
        };

        let mut conn = state.pool.get().await?;
        let client = conn.client();

        let mut query = claw::Query::new(full_sql);
        for val in &source_values {
            query.bind(val.as_str());
        }

        let stream = query
            .query(client)
            .await
            .map_err(|e| Error::Sql(e.to_string()))?;

        let embed_rows = stream
            .into_first_result()
            .await
            .map_err(|e| Error::Sql(e.to_string()))?;

        let embed_json: Vec<serde_json::Map<String, JsonValue>> =
            embed_rows.iter().map(types::row_to_json).collect();

        // Group embed results by the join column
        let mut grouped: HashMap<String, Vec<JsonValue>> = HashMap::new();
        for erow in &embed_json {
            if let Some(key_val) = erow.get(&embed_info.target_column) {
                let key = match key_val {
                    JsonValue::String(s) => s.clone(),
                    JsonValue::Null => continue,
                    other => other.to_string(),
                };
                grouped
                    .entry(key)
                    .or_default()
                    .push(JsonValue::Object(erow.clone()));
            }
        }

        // Attach to parent rows
        for row in rows.iter_mut() {
            let source_val = row
                .get(&embed_info.source_column)
                .map(|v| match v {
                    JsonValue::String(s) => s.clone(),
                    JsonValue::Null => String::new(),
                    other => other.to_string(),
                })
                .unwrap_or_default();

            let embedded = grouped
                .get(&source_val)
                .cloned()
                .unwrap_or_default();

            match embed_info.join_type {
                crate::schema::EmbedJoinType::ManyToOne => {
                    // Many-to-one: embed as single object or null
                    if let Some(first) = embedded.into_iter().next() {
                        row.insert(embed.name.clone(), first);
                    } else {
                        row.insert(embed.name.clone(), JsonValue::Null);
                    }
                }
                crate::schema::EmbedJoinType::OneToMany => {
                    row.insert(embed.name.clone(), JsonValue::Array(embedded));
                }
            }
        }
    }

    Ok(())
}

/// Build column list for an embed query.
fn build_embed_column_list(
    table: &crate::schema::TableInfo,
    nodes: &[SelectNode],
) -> String {
    if nodes.is_empty() || select::has_star(nodes) {
        table
            .columns
            .iter()
            .map(|c| format!("[{}]", escape_ident(&c.name)))
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        let cols = select::select_columns(nodes);
        if cols.is_empty() {
            "*".to_string()
        } else {
            cols.iter()
                .map(|c| format!("[{}]", escape_ident(c)))
                .collect::<Vec<_>>()
                .join(", ")
        }
    }
}

/// Parse Range header: "0-24" -> (Some(25), Some(0))
fn parse_range_header(headers: &HeaderMap) -> (Option<i64>, Option<i64>) {
    if let Some(range) = headers.get("range").and_then(|v| v.to_str().ok()) {
        let parts: Vec<&str> = range.split('-').collect();
        if parts.len() == 2 {
            if let (Ok(start), Ok(end)) = (parts[0].parse::<i64>(), parts[1].parse::<i64>()) {
                let limit = end - start + 1;
                return (Some(limit), Some(start));
            }
        }
    }
    (None, None)
}

/// Build Content-Range header value.
fn build_content_range(offset: i64, count: i64, total: Option<i64>) -> String {
    let end = if count > 0 {
        offset + count - 1
    } else {
        offset
    };
    let total_str = total
        .map(|t| t.to_string())
        .unwrap_or_else(|| "*".to_string());
    format!("{}-{}/{}", offset, end, total_str)
}

/// Convert a JSON value to a string suitable for SQL parameter binding.
fn json_value_to_sql_string(val: &JsonValue) -> String {
    match val {
        JsonValue::Null => String::new(), // Will be bound as empty string
        JsonValue::Bool(b) => {
            if *b {
                "1".to_string()
            } else {
                "0".to_string()
            }
        }
        JsonValue::Number(n) => n.to_string(),
        JsonValue::String(s) => s.clone(),
        JsonValue::Array(arr) => serde_json::to_string(arr).unwrap_or_default(),
        JsonValue::Object(obj) => serde_json::to_string(obj).unwrap_or_default(),
    }
}
