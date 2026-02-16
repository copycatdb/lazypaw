//! OpenAPI 3.0 spec auto-generation from schema introspection.

use crate::config::AppConfig;
use crate::schema::{SchemaCache, TableInfo};
use crate::types;
use serde_json::{json, Map, Value};

/// Generate the OpenAPI 3.0 specification.
pub fn generate_openapi(schema: &SchemaCache, config: &AppConfig) -> Value {
    let mut paths = Map::new();
    let mut schemas = Map::new();

    for ((schema_name, _table_name), table) in &schema.tables {
        let path = if schema_name.eq_ignore_ascii_case(&config.default_schema) {
            format!("/{}", table.name)
        } else {
            format!("/{}/{}", schema_name, table.name)
        };

        let (path_item, table_schema) = generate_table_paths(table, config);
        paths.insert(path.clone(), path_item);
        schemas.insert(table.name.clone(), table_schema);
    }

    // Add RPC path template
    paths.insert(
        "/rpc/{procedure}".to_string(),
        json!({
            "post": {
                "summary": "Execute stored procedure",
                "parameters": [{
                    "name": "procedure",
                    "in": "path",
                    "required": true,
                    "schema": { "type": "string" }
                }],
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "object",
                                "additionalProperties": true
                            }
                        }
                    }
                },
                "responses": {
                    "200": {
                        "description": "Procedure executed",
                        "content": {
                            "application/json": {
                                "schema": { "type": "array", "items": { "type": "object" } }
                            }
                        }
                    }
                }
            }
        }),
    );

    json!({
        "openapi": "3.0.3",
        "info": {
            "title": format!("lazypaw API — {}", config.database.as_deref().unwrap_or("SQL Server")),
            "description": "Auto-generated REST API from SQL Server schema",
            "version": "0.1.0"
        },
        "servers": [{
            "url": format!("http://localhost:{}", config.listen_port)
        }],
        "paths": paths,
        "components": {
            "schemas": schemas,
            "securitySchemes": {
                "bearerAuth": {
                    "type": "http",
                    "scheme": "bearer",
                    "bearerFormat": "JWT"
                }
            }
        }
    })
}

/// Generate OpenAPI path item and schema for a table.
fn generate_table_paths(table: &TableInfo, _config: &AppConfig) -> (Value, Value) {
    let schema_ref = format!("#/components/schemas/{}", table.name);

    // Build table schema
    let mut properties = Map::new();
    let mut required = Vec::new();

    for col in &table.columns {
        let (type_str, format_str) = types::sql_type_to_openapi(&col.data_type);
        let mut prop = Map::new();
        prop.insert("type".to_string(), json!(type_str));
        if !format_str.is_empty() {
            prop.insert("format".to_string(), json!(format_str));
        }
        if col.is_nullable {
            prop.insert("nullable".to_string(), json!(true));
        }
        if col.is_identity {
            prop.insert("readOnly".to_string(), json!(true));
        }
        properties.insert(col.name.clone(), Value::Object(prop));

        if !col.is_nullable && !col.is_identity && !col.has_default {
            required.push(json!(col.name));
        }
    }

    let table_schema = json!({
        "type": "object",
        "properties": properties,
        "required": required
    });

    // Build filter parameters
    let mut filter_params: Vec<Value> = Vec::new();

    // Standard PostgREST params
    filter_params.push(json!({
        "name": "select",
        "in": "query",
        "description": "Column selection (e.g., col1,col2,related(*))",
        "schema": { "type": "string" }
    }));
    filter_params.push(json!({
        "name": "order",
        "in": "query",
        "description": "Ordering (e.g., name.asc,age.desc)",
        "schema": { "type": "string" }
    }));
    filter_params.push(json!({
        "name": "limit",
        "in": "query",
        "description": "Maximum number of rows",
        "schema": { "type": "integer" }
    }));
    filter_params.push(json!({
        "name": "offset",
        "in": "query",
        "description": "Number of rows to skip",
        "schema": { "type": "integer" }
    }));

    // Per-column filter params
    for col in &table.columns {
        filter_params.push(json!({
            "name": col.name,
            "in": "query",
            "description": format!("Filter on {} (e.g., eq.value, gt.5, in.(a,b))", col.name),
            "schema": { "type": "string" }
        }));
    }

    let mut path_item = Map::new();

    // GET
    path_item.insert(
        "get".to_string(),
        json!({
            "summary": format!("Read {}", table.name),
            "parameters": filter_params,
            "responses": {
                "200": {
                    "description": format!("List of {}", table.name),
                    "content": {
                        "application/json": {
                            "schema": {
                                "type": "array",
                                "items": { "$ref": schema_ref }
                            }
                        },
                        "text/csv": {
                            "schema": { "type": "string" }
                        },
                        "application/vnd.pgrst.object+json": {
                            "schema": { "$ref": schema_ref }
                        }
                    },
                    "headers": {
                        "Content-Range": {
                            "schema": { "type": "string" },
                            "description": "Pagination range"
                        }
                    }
                }
            }
        }),
    );

    // POST (not for views)
    if !table.is_view {
        path_item.insert(
            "post".to_string(),
            json!({
                "summary": format!("Insert into {}", table.name),
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": {
                                "oneOf": [
                                    { "$ref": schema_ref },
                                    { "type": "array", "items": { "$ref": schema_ref } }
                                ]
                            }
                        }
                    }
                },
                "responses": {
                    "201": {
                        "description": "Created",
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "array",
                                    "items": { "$ref": schema_ref }
                                }
                            }
                        }
                    }
                }
            }),
        );

        // PATCH
        path_item.insert(
            "patch".to_string(),
            json!({
                "summary": format!("Update {}", table.name),
                "parameters": filter_params,
                "requestBody": {
                    "content": {
                        "application/json": {
                            "schema": { "$ref": schema_ref }
                        }
                    }
                },
                "responses": {
                    "200": {
                        "description": "Updated",
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "array",
                                    "items": { "$ref": schema_ref }
                                }
                            }
                        }
                    }
                }
            }),
        );

        // DELETE
        path_item.insert(
            "delete".to_string(),
            json!({
                "summary": format!("Delete from {}", table.name),
                "parameters": filter_params,
                "responses": {
                    "200": {
                        "description": "Deleted",
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "array",
                                    "items": { "$ref": schema_ref }
                                }
                            }
                        }
                    }
                }
            }),
        );
    }

    (Value::Object(path_item), table_schema)
}

/// Generate a simple Swagger UI HTML page.
pub fn swagger_ui_html(listen_port: u16) -> String {
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>lazypaw API</title>
    <meta charset="utf-8"/>
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <link rel="stylesheet" type="text/css" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css">
</head>
<body>
    <div id="swagger-ui"></div>
    <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
    <script>
        SwaggerUIBundle({{
            url: "http://localhost:{}/",
            dom_id: '#swagger-ui',
            presets: [
                SwaggerUIBundle.presets.apis,
                SwaggerUIBundle.SwaggerUIStandalonePreset
            ],
            layout: "BaseLayout"
        }})
    </script>
</body>
</html>"#,
        listen_port
    )
}
