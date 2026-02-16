#![allow(dead_code)]
//! Realtime change notification engine using SQL Server Change Tracking.

use crate::config::AppConfig;
use crate::filters::{self, Filter, FilterOp, FilterValue};
use crate::pool::Pool;
use crate::query::escape_ident;
use crate::schema::SchemaCache;
use crate::types;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ChangeOp {
    Insert,
    Update,
    Delete,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangeEvent {
    #[serde(rename = "type")]
    pub type_: String,
    pub id: String,
    pub table: String,
    pub record: serde_json::Map<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ClientMessage {
    Subscribe {
        id: String,
        table: String,
        #[serde(default)]
        filter: Option<String>,
        #[serde(default)]
        events: Option<Vec<String>>,
    },
    Unsubscribe {
        id: String,
    },
    Ping,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ServerMessage {
    Subscribed {
        #[serde(rename = "type")]
        type_: &'static str,
        id: String,
        table: String,
    },
    Unsubscribed {
        #[serde(rename = "type")]
        type_: &'static str,
        id: String,
    },
    Error {
        #[serde(rename = "type")]
        type_: &'static str,
        message: String,
    },
    Pong {
        #[serde(rename = "type")]
        type_: &'static str,
    },
    Change {
        #[serde(rename = "type")]
        type_: String,
        id: String,
        table: String,
        record: serde_json::Map<String, JsonValue>,
    },
}

struct Subscription {
    id: String,
    table_key: String,
    client_tx: mpsc::Sender<ServerMessage>,
    filter: Option<Vec<Filter>>,
    events: HashSet<ChangeOp>,
}

pub struct RealtimeEngine {
    table_subs: RwLock<HashMap<String, Vec<Uuid>>>,
    all_subs: RwLock<HashMap<Uuid, Subscription>>,
    client_subs: RwLock<HashMap<Uuid, Vec<Uuid>>>,
    last_version: AtomicI64,
    pool: Arc<Pool>,
    schema: Arc<RwLock<SchemaCache>>,
    config: AppConfig,
}

impl RealtimeEngine {
    pub fn new(pool: Arc<Pool>, schema: Arc<RwLock<SchemaCache>>, config: AppConfig) -> Arc<Self> {
        Arc::new(Self {
            table_subs: RwLock::new(HashMap::new()),
            all_subs: RwLock::new(HashMap::new()),
            client_subs: RwLock::new(HashMap::new()),
            last_version: AtomicI64::new(-1),
            pool,
            schema,
            config,
        })
    }

    pub async fn subscribe(
        &self,
        client_id: Uuid,
        sub_id: String,
        table: &str,
        filter_str: Option<&str>,
        events: Option<Vec<String>>,
        tx: mpsc::Sender<ServerMessage>,
    ) -> Result<String, String> {
        let schema_cache = self.schema.read().await;

        let (schema_name, table_name) = if table.contains('.') {
            let parts: Vec<&str> = table.splitn(2, '.').collect();
            (parts[0].to_string(), parts[1].to_string())
        } else {
            (self.config.default_schema.clone(), table.to_string())
        };

        let table_key = format!("{}.{}", schema_name, table_name);

        let table_info = schema_cache
            .get_table(&schema_name, &table_name)
            .ok_or_else(|| format!("Table not found: {}", table_key))?;

        if !table_info.change_tracking_enabled {
            return Err(format!("Change tracking not enabled on {}", table_key));
        }

        // Parse filters
        let parsed_filters = if let Some(f) = filter_str {
            let mut fv = Vec::new();
            for part in f.split('&') {
                if let Some((key, val)) = part.split_once('=') {
                    match filters::parse_filter(key, val) {
                        Ok(filter) => fv.push(filter),
                        Err(e) => return Err(format!("Invalid filter: {}", e)),
                    }
                }
            }
            Some(fv)
        } else {
            None
        };

        let event_set: HashSet<ChangeOp> = if let Some(evts) = events {
            evts.iter()
                .map(|e| match e.to_uppercase().as_str() {
                    "INSERT" => ChangeOp::Insert,
                    "UPDATE" => ChangeOp::Update,
                    "DELETE" => ChangeOp::Delete,
                    _ => ChangeOp::Insert,
                })
                .collect()
        } else {
            [ChangeOp::Insert, ChangeOp::Update, ChangeOp::Delete]
                .into_iter()
                .collect()
        };

        let sub_uuid = Uuid::new_v4();
        let sub = Subscription {
            id: sub_id,
            table_key: table_key.clone(),
            client_tx: tx,
            filter: parsed_filters,
            events: event_set,
        };

        self.all_subs.write().await.insert(sub_uuid, sub);
        self.table_subs
            .write()
            .await
            .entry(table_key.clone())
            .or_default()
            .push(sub_uuid);
        self.client_subs
            .write()
            .await
            .entry(client_id)
            .or_default()
            .push(sub_uuid);

        Ok(table_key)
    }

    pub async fn unsubscribe(&self, client_id: Uuid, sub_id: &str) {
        let client_sub_uuids = self
            .client_subs
            .read()
            .await
            .get(&client_id)
            .cloned()
            .unwrap_or_default();
        let mut to_remove = None;
        for uuid in &client_sub_uuids {
            if let Some(sub) = self.all_subs.read().await.get(uuid) {
                if sub.id == sub_id {
                    to_remove = Some((*uuid, sub.table_key.clone()));
                    break;
                }
            }
        }
        if let Some((uuid, table_key)) = to_remove {
            self.all_subs.write().await.remove(&uuid);
            if let Some(subs) = self.table_subs.write().await.get_mut(&table_key) {
                subs.retain(|u| *u != uuid);
            }
            if let Some(subs) = self.client_subs.write().await.get_mut(&client_id) {
                subs.retain(|u| *u != uuid);
            }
        }
    }

    pub async fn remove_client(&self, client_id: Uuid) {
        let sub_uuids = self
            .client_subs
            .write()
            .await
            .remove(&client_id)
            .unwrap_or_default();
        for uuid in sub_uuids {
            if let Some(sub) = self.all_subs.write().await.remove(&uuid) {
                if let Some(subs) = self.table_subs.write().await.get_mut(&sub.table_key) {
                    subs.retain(|u| *u != uuid);
                }
            }
        }
    }

    pub async fn init_version(&self) -> Result<(), String> {
        let mut conn = self.pool.get().await.map_err(|e| e.to_string())?;
        let client = conn.client();
        let stream = claw::Query::new("SELECT CHANGE_TRACKING_CURRENT_VERSION()")
            .query(client)
            .await
            .map_err(|e| e.to_string())?;
        let rows = stream
            .into_first_result()
            .await
            .map_err(|e| e.to_string())?;
        if let Some(row) = rows.first() {
            let json = types::row_to_json(row);
            if let Some((_, val)) = json.into_iter().next() {
                match val {
                    JsonValue::Number(n) => {
                        if let Some(v) = n.as_i64() {
                            self.last_version.store(v, Ordering::SeqCst);
                        }
                    }
                    JsonValue::Null => {
                        self.last_version.store(0, Ordering::SeqCst);
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    pub async fn poll_loop(self: Arc<Self>, poll_ms: u64) {
        loop {
            if let Err(e) = self.poll_once().await {
                tracing::error!("Realtime poll error: {}", e);
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(poll_ms)).await;
        }
    }

    async fn poll_once(&self) -> Result<(), String> {
        let active_tables: Vec<String> = {
            let table_subs = self.table_subs.read().await;
            table_subs
                .iter()
                .filter(|(_, subs)| !subs.is_empty())
                .map(|(k, _)| k.clone())
                .collect()
        };

        if active_tables.is_empty() {
            return Ok(());
        }

        let mut conn = self.pool.get().await.map_err(|e| e.to_string())?;
        let client = conn.client();

        // Get current version
        let stream = claw::Query::new("SELECT CHANGE_TRACKING_CURRENT_VERSION()")
            .query(client)
            .await
            .map_err(|e| e.to_string())?;
        let version_rows = stream
            .into_first_result()
            .await
            .map_err(|e| e.to_string())?;

        let current_version = if let Some(row) = version_rows.first() {
            let json = types::row_to_json(row);
            if let Some((_, JsonValue::Number(n))) = json.into_iter().next() {
                n.as_i64().unwrap_or(0)
            } else {
                return Ok(());
            }
        } else {
            return Ok(());
        };

        let last = self.last_version.load(Ordering::SeqCst);
        if current_version <= last {
            return Ok(());
        }

        let schema_cache = self.schema.read().await;

        for table_key in &active_tables {
            let parts: Vec<&str> = table_key.splitn(2, '.').collect();
            if parts.len() != 2 {
                continue;
            }
            let (schema_name, table_name) = (parts[0], parts[1]);

            let table_info = match schema_cache.get_table(schema_name, table_name) {
                Some(t) => t,
                None => continue,
            };

            if table_info.primary_key.is_empty() {
                continue;
            }

            let pk_join = table_info
                .primary_key
                .iter()
                .map(|pk| format!("t.[{}] = ct.[{}]", escape_ident(pk), escape_ident(pk)))
                .collect::<Vec<_>>()
                .join(" AND ");

            let all_cols = table_info
                .columns
                .iter()
                .map(|c| format!("t.[{}]", escape_ident(&c.name)))
                .collect::<Vec<_>>()
                .join(", ");

            let ct_pk_cols = table_info
                .primary_key
                .iter()
                .map(|pk| format!("ct.[{}] AS [__ct_{}]", escape_ident(pk), escape_ident(pk)))
                .collect::<Vec<_>>()
                .join(", ");

            let sql = format!(
                "SELECT ct.SYS_CHANGE_OPERATION, ct.SYS_CHANGE_VERSION, {}, {} \
                 FROM CHANGETABLE(CHANGES [{}].[{}], @P1) AS ct \
                 LEFT JOIN [{}].[{}] t ON {}",
                ct_pk_cols,
                all_cols,
                escape_ident(schema_name),
                escape_ident(table_name),
                escape_ident(schema_name),
                escape_ident(table_name),
                pk_join
            );

            let mut query = claw::Query::new(&sql);
            query.bind(last);
            let stream = match query.query(client).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("CT query failed for {}: {}", table_key, e);
                    continue;
                }
            };
            let rows = match stream.into_first_result().await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("CT result failed for {}: {}", table_key, e);
                    continue;
                }
            };

            for row in &rows {
                let row_json = types::row_to_json(row);

                // Get operation
                let op = match row_json.get("SYS_CHANGE_OPERATION") {
                    Some(JsonValue::String(s)) => match s.as_str() {
                        "I" => ChangeOp::Insert,
                        "U" => ChangeOp::Update,
                        "D" => ChangeOp::Delete,
                        _ => continue,
                    },
                    _ => continue,
                };

                // Build record (exclude CT internal columns)
                let mut record = serde_json::Map::new();
                if op == ChangeOp::Delete {
                    // For DELETE, use ct PK columns
                    for (k, v) in &row_json {
                        if let Some(pk_name) = k.strip_prefix("__ct_") {
                            record.insert(pk_name.to_string(), v.clone());
                        }
                    }
                } else {
                    for (k, v) in &row_json {
                        if !k.starts_with("SYS_CHANGE_") && !k.starts_with("__ct_") {
                            record.insert(k.clone(), v.clone());
                        }
                    }
                }

                // Fan out to subscriptions
                let sub_uuids = self
                    .table_subs
                    .read()
                    .await
                    .get(table_key)
                    .cloned()
                    .unwrap_or_default();

                let all_subs = self.all_subs.read().await;
                for sub_uuid in &sub_uuids {
                    if let Some(sub) = all_subs.get(sub_uuid) {
                        if !sub.events.contains(&op) {
                            continue;
                        }

                        if let Some(ref filter_list) = sub.filter {
                            let mut matches = true;
                            for filter in filter_list {
                                if let Some(val) = record.get(&filter.column) {
                                    if !filter_matches(filter, val) {
                                        matches = false;
                                        break;
                                    }
                                }
                            }
                            if !matches {
                                continue;
                            }
                        }

                        let op_str = match op {
                            ChangeOp::Insert => "INSERT",
                            ChangeOp::Update => "UPDATE",
                            ChangeOp::Delete => "DELETE",
                        };

                        let msg = ServerMessage::Change {
                            type_: op_str.to_string(),
                            id: sub.id.clone(),
                            table: table_key.clone(),
                            record: record.clone(),
                        };

                        let _ = sub.client_tx.try_send(msg);
                    }
                }
            }
        }

        self.last_version.store(current_version, Ordering::SeqCst);
        Ok(())
    }
}

fn filter_matches(filter: &Filter, value: &JsonValue) -> bool {
    let val_str = match value {
        JsonValue::String(s) => s.clone(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Null => "null".to_string(),
        other => other.to_string(),
    };

    let result = match &filter.operator {
        FilterOp::Eq => match &filter.value {
            FilterValue::Single(expected) => val_str == *expected,
            _ => true,
        },
        FilterOp::Neq => match &filter.value {
            FilterValue::Single(expected) => val_str != *expected,
            _ => true,
        },
        FilterOp::In => match &filter.value {
            FilterValue::List(items) => items.contains(&val_str),
            _ => true,
        },
        FilterOp::Is => match &filter.value {
            FilterValue::Single(expected) => match expected.to_lowercase().as_str() {
                "null" => value.is_null(),
                "true" => value == &JsonValue::Bool(true),
                "false" => value == &JsonValue::Bool(false),
                _ => true,
            },
            _ => true,
        },
        // For comparison ops, try numeric comparison
        FilterOp::Gt | FilterOp::Gte | FilterOp::Lt | FilterOp::Lte => {
            match &filter.value {
                FilterValue::Single(expected) => {
                    if let (Ok(a), Ok(b)) = (val_str.parse::<f64>(), expected.parse::<f64>()) {
                        match &filter.operator {
                            FilterOp::Gt => a > b,
                            FilterOp::Gte => a >= b,
                            FilterOp::Lt => a < b,
                            FilterOp::Lte => a <= b,
                            _ => true,
                        }
                    } else {
                        // String comparison fallback
                        match &filter.operator {
                            FilterOp::Gt => val_str > *expected,
                            FilterOp::Gte => val_str >= *expected,
                            FilterOp::Lt => val_str < *expected,
                            FilterOp::Lte => val_str <= *expected,
                            _ => true,
                        }
                    }
                }
                _ => true,
            }
        }
        _ => true, // Like, Ilike, Fts — pass through
    };

    if filter.negated {
        !result
    } else {
        result
    }
}
