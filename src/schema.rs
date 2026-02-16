#![allow(dead_code)]
//! Schema introspection & in-memory model.
//!
//! Reads tables, views, columns, types, PKs, FKs, and unique constraints
//! from INFORMATION_SCHEMA and sys.* catalog views on startup (and on SIGHUP).

use crate::error::Error;
use crate::pool::Pool;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

/// A column in a table or view.
#[derive(Debug, Clone, Serialize)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub max_length: Option<i32>,
    pub precision: Option<i32>,
    pub scale: Option<i32>,
    pub is_nullable: bool,
    pub ordinal_position: i32,
    pub is_identity: bool,
    pub has_default: bool,
    pub is_computed: bool,
}

/// A foreign key relationship.
#[derive(Debug, Clone, Serialize)]
pub struct ForeignKey {
    pub constraint_name: String,
    pub column_name: String,
    pub ref_schema: String,
    pub ref_table: String,
    pub ref_column: String,
}

/// A table or view in the schema.
#[derive(Debug, Clone, Serialize)]
pub struct TableInfo {
    pub name: String,
    pub schema: String,
    pub columns: Vec<ColumnInfo>,
    pub primary_key: Vec<String>,
    pub foreign_keys: Vec<ForeignKey>,
    pub unique_constraints: Vec<Vec<String>>,
    pub is_view: bool,
    pub change_tracking_enabled: bool,
}

impl TableInfo {
    /// Full qualified name: [schema].[table]
    pub fn full_name(&self) -> String {
        format!("[{}].[{}]", self.schema, self.name)
    }

    /// Get column info by name.
    pub fn column(&self, name: &str) -> Option<&ColumnInfo> {
        self.columns
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
    }

    /// Columns that can be used in INSERT (non-identity, non-computed).
    pub fn insertable_columns(&self) -> Vec<&ColumnInfo> {
        self.columns.iter().filter(|c| !c.is_identity).collect()
    }
}

/// The complete schema model loaded from the database.
#[derive(Debug, Clone)]
pub struct SchemaCache {
    /// Key: (schema, table_name) -> TableInfo
    pub tables: HashMap<(String, String), TableInfo>,
    /// Reverse FK index: (ref_schema, ref_table) -> list of tables that reference it
    pub reverse_fks: HashMap<(String, String), Vec<(String, String, ForeignKey)>>,
}

impl SchemaCache {
    /// Look up a table by schema and name (case-insensitive).
    pub fn get_table(&self, schema: &str, table: &str) -> Option<&TableInfo> {
        // Try exact match first
        if let Some(t) = self.tables.get(&(schema.to_string(), table.to_string())) {
            return Some(t);
        }
        // Case-insensitive search
        self.tables.iter().find_map(|((s, t), info)| {
            if s.eq_ignore_ascii_case(schema) && t.eq_ignore_ascii_case(table) {
                Some(info)
            } else {
                None
            }
        })
    }

    /// Find tables that reference the given table (reverse FK lookup).
    pub fn referencing_tables(
        &self,
        schema: &str,
        table: &str,
    ) -> Vec<&(String, String, ForeignKey)> {
        let key = (schema.to_lowercase(), table.to_lowercase());
        self.reverse_fks
            .get(&key)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    /// Find FK from source table to target table by embed name.
    pub fn find_embed(
        &self,
        source_schema: &str,
        source_table: &str,
        embed_name: &str,
        hint_fk: Option<&str>,
    ) -> Option<EmbedInfo> {
        let source = self.get_table(source_schema, source_table)?;

        // 1. Check if embed_name matches a table that source has an FK to
        for fk in &source.foreign_keys {
            if fk.ref_table.eq_ignore_ascii_case(embed_name) {
                if let Some(hint) = hint_fk {
                    if !fk.constraint_name.eq_ignore_ascii_case(hint) {
                        continue;
                    }
                }
                return Some(EmbedInfo {
                    target_schema: fk.ref_schema.clone(),
                    target_table: fk.ref_table.clone(),
                    join_type: EmbedJoinType::ManyToOne,
                    source_column: fk.column_name.clone(),
                    target_column: fk.ref_column.clone(),
                });
            }
        }

        // 2. Check reverse FKs — tables that have FK pointing to source
        let refs = self.referencing_tables(source_schema, source_table);
        for (ref_schema, ref_table, fk) in refs {
            if ref_table.eq_ignore_ascii_case(embed_name) {
                if let Some(hint) = hint_fk {
                    if !fk.constraint_name.eq_ignore_ascii_case(hint) {
                        continue;
                    }
                }
                return Some(EmbedInfo {
                    target_schema: ref_schema.clone(),
                    target_table: ref_table.clone(),
                    join_type: EmbedJoinType::OneToMany,
                    source_column: fk.ref_column.clone(),
                    target_column: fk.column_name.clone(),
                });
            }
        }

        None
    }
    /// Check if all tables belong to a single schema.
    pub fn has_multiple_schemas(&self) -> bool {
        let mut schemas = std::collections::HashSet::new();
        for (schema, _) in self.tables.keys() {
            schemas.insert(schema.to_lowercase());
        }
        schemas.len() > 1
    }
}

/// Info about how to embed a related table.
#[derive(Debug, Clone)]
pub struct EmbedInfo {
    pub target_schema: String,
    pub target_table: String,
    pub join_type: EmbedJoinType,
    pub source_column: String,
    pub target_column: String,
}

#[derive(Debug, Clone)]
pub enum EmbedJoinType {
    ManyToOne,
    OneToMany,
}

/// Load the full schema from the database.
pub async fn load_schema(pool: &Arc<Pool>) -> Result<SchemaCache, Error> {
    let mut conn = pool.get().await?;
    let client = conn.client();

    // 1. Load tables and views
    let table_rows = client
        .execute(
            "SELECT TABLE_SCHEMA, TABLE_NAME, TABLE_TYPE \
             FROM INFORMATION_SCHEMA.TABLES \
             ORDER BY TABLE_SCHEMA, TABLE_NAME",
            &[],
        )
        .await
        .map_err(|e| Error::Sql(e.to_string()))?
        .into_first_result()
        .await
        .map_err(|e| Error::Sql(e.to_string()))?;

    let mut tables = HashMap::new();

    for row in &table_rows {
        let schema: &str = row.get("TABLE_SCHEMA").unwrap_or("dbo");
        let name: &str = row.get("TABLE_NAME").unwrap_or("");
        let ttype: &str = row.get("TABLE_TYPE").unwrap_or("BASE TABLE");

        let is_view = ttype.contains("VIEW");

        tables.insert(
            (schema.to_string(), name.to_string()),
            TableInfo {
                name: name.to_string(),
                schema: schema.to_string(),
                columns: Vec::new(),
                primary_key: Vec::new(),
                foreign_keys: Vec::new(),
                unique_constraints: Vec::new(),
                is_view,
                change_tracking_enabled: false,
            },
        );
    }

    // 2. Load columns with identity info
    let col_rows = client
        .execute(
            "SELECT c.TABLE_SCHEMA, c.TABLE_NAME, c.COLUMN_NAME, c.DATA_TYPE, \
                    c.CHARACTER_MAXIMUM_LENGTH, c.NUMERIC_PRECISION, c.NUMERIC_SCALE, \
                    c.IS_NULLABLE, c.ORDINAL_POSITION, c.COLUMN_DEFAULT, \
                    COLUMNPROPERTY(OBJECT_ID(c.TABLE_SCHEMA + '.' + c.TABLE_NAME), c.COLUMN_NAME, 'IsIdentity') AS IS_IDENTITY, \
                    COLUMNPROPERTY(OBJECT_ID(c.TABLE_SCHEMA + '.' + c.TABLE_NAME), c.COLUMN_NAME, 'IsComputed') AS IS_COMPUTED \
             FROM INFORMATION_SCHEMA.COLUMNS c \
             ORDER BY c.TABLE_SCHEMA, c.TABLE_NAME, c.ORDINAL_POSITION",
            &[],
        )
        .await
        .map_err(|e| Error::Sql(e.to_string()))?
        .into_first_result()
        .await
        .map_err(|e| Error::Sql(e.to_string()))?;

    for row in &col_rows {
        let schema: &str = row.get("TABLE_SCHEMA").unwrap_or("dbo");
        let table: &str = row.get("TABLE_NAME").unwrap_or("");
        let col_name: &str = row.get("COLUMN_NAME").unwrap_or("");
        let data_type: &str = row.get("DATA_TYPE").unwrap_or("nvarchar");
        let max_len: Option<i32> = row.get("CHARACTER_MAXIMUM_LENGTH");
        let precision: Option<i32> = row
            .try_get::<u8, _>("NUMERIC_PRECISION")
            .ok()
            .flatten()
            .map(|v| v as i32);
        let scale: Option<i32> = row.try_get::<i32, _>("NUMERIC_SCALE").ok().flatten();
        let is_nullable: &str = row.get("IS_NULLABLE").unwrap_or("YES");
        let ordinal: i32 = row.get("ORDINAL_POSITION").unwrap_or(0);
        let is_identity: i32 = row.get("IS_IDENTITY").unwrap_or(0);
        let is_computed: i32 = row.get("IS_COMPUTED").unwrap_or(0);
        let has_default = row
            .try_get::<&str, _>("COLUMN_DEFAULT")
            .ok()
            .flatten()
            .is_some();

        let key = (schema.to_string(), table.to_string());
        if let Some(table_info) = tables.get_mut(&key) {
            table_info.columns.push(ColumnInfo {
                name: col_name.to_string(),
                data_type: data_type.to_string(),
                max_length: max_len,
                precision,
                scale,
                is_nullable: is_nullable == "YES",
                ordinal_position: ordinal,
                is_identity: is_identity == 1,
                has_default,
                is_computed: is_computed == 1,
            });
        }
    }

    // 3. Load primary keys
    let pk_rows = client
        .execute(
            "SELECT ku.TABLE_SCHEMA, ku.TABLE_NAME, ku.COLUMN_NAME \
             FROM INFORMATION_SCHEMA.TABLE_CONSTRAINTS tc \
             JOIN INFORMATION_SCHEMA.KEY_COLUMN_USAGE ku \
                 ON tc.CONSTRAINT_NAME = ku.CONSTRAINT_NAME \
                 AND tc.TABLE_SCHEMA = ku.TABLE_SCHEMA \
             WHERE tc.CONSTRAINT_TYPE = 'PRIMARY KEY' \
             ORDER BY ku.TABLE_SCHEMA, ku.TABLE_NAME, ku.ORDINAL_POSITION",
            &[],
        )
        .await
        .map_err(|e| Error::Sql(e.to_string()))?
        .into_first_result()
        .await
        .map_err(|e| Error::Sql(e.to_string()))?;

    for row in &pk_rows {
        let schema: &str = row.get("TABLE_SCHEMA").unwrap_or("dbo");
        let table: &str = row.get("TABLE_NAME").unwrap_or("");
        let col: &str = row.get("COLUMN_NAME").unwrap_or("");

        let key = (schema.to_string(), table.to_string());
        if let Some(table_info) = tables.get_mut(&key) {
            table_info.primary_key.push(col.to_string());
        }
    }

    // 4. Load foreign keys
    let fk_rows = client
        .execute(
            "SELECT \
                 fk.name AS FK_NAME, \
                 OBJECT_SCHEMA_NAME(fkc.parent_object_id) AS TABLE_SCHEMA, \
                 OBJECT_NAME(fkc.parent_object_id) AS TABLE_NAME, \
                 COL_NAME(fkc.parent_object_id, fkc.parent_column_id) AS COLUMN_NAME, \
                 OBJECT_SCHEMA_NAME(fkc.referenced_object_id) AS REF_SCHEMA, \
                 OBJECT_NAME(fkc.referenced_object_id) AS REF_TABLE, \
                 COL_NAME(fkc.referenced_object_id, fkc.referenced_column_id) AS REF_COLUMN \
             FROM sys.foreign_keys fk \
             JOIN sys.foreign_key_columns fkc ON fk.object_id = fkc.constraint_object_id \
             ORDER BY fk.name",
            &[],
        )
        .await
        .map_err(|e| Error::Sql(e.to_string()))?
        .into_first_result()
        .await
        .map_err(|e| Error::Sql(e.to_string()))?;

    let mut reverse_fks: HashMap<(String, String), Vec<(String, String, ForeignKey)>> =
        HashMap::new();

    for row in &fk_rows {
        let fk_name: &str = row.get("FK_NAME").unwrap_or("");
        let schema: &str = row.get("TABLE_SCHEMA").unwrap_or("dbo");
        let table: &str = row.get("TABLE_NAME").unwrap_or("");
        let col: &str = row.get("COLUMN_NAME").unwrap_or("");
        let ref_schema: &str = row.get("REF_SCHEMA").unwrap_or("dbo");
        let ref_table: &str = row.get("REF_TABLE").unwrap_or("");
        let ref_col: &str = row.get("REF_COLUMN").unwrap_or("");

        let fk = ForeignKey {
            constraint_name: fk_name.to_string(),
            column_name: col.to_string(),
            ref_schema: ref_schema.to_string(),
            ref_table: ref_table.to_string(),
            ref_column: ref_col.to_string(),
        };

        let key = (schema.to_string(), table.to_string());
        if let Some(table_info) = tables.get_mut(&key) {
            table_info.foreign_keys.push(fk.clone());
        }

        // Reverse FK index
        let ref_key = (ref_schema.to_lowercase(), ref_table.to_lowercase());
        reverse_fks
            .entry(ref_key)
            .or_default()
            .push((schema.to_string(), table.to_string(), fk));
    }

    // 5. Load unique constraints
    let uq_rows = client
        .execute(
            "SELECT tc.TABLE_SCHEMA, tc.TABLE_NAME, tc.CONSTRAINT_NAME, ku.COLUMN_NAME \
             FROM INFORMATION_SCHEMA.TABLE_CONSTRAINTS tc \
             JOIN INFORMATION_SCHEMA.KEY_COLUMN_USAGE ku \
                 ON tc.CONSTRAINT_NAME = ku.CONSTRAINT_NAME \
                 AND tc.TABLE_SCHEMA = ku.TABLE_SCHEMA \
             WHERE tc.CONSTRAINT_TYPE = 'UNIQUE' \
             ORDER BY tc.TABLE_SCHEMA, tc.TABLE_NAME, tc.CONSTRAINT_NAME, ku.ORDINAL_POSITION",
            &[],
        )
        .await
        .map_err(|e| Error::Sql(e.to_string()))?
        .into_first_result()
        .await
        .map_err(|e| Error::Sql(e.to_string()))?;

    let mut uq_map: HashMap<(String, String, String), Vec<String>> = HashMap::new();
    for row in &uq_rows {
        let schema: &str = row.get("TABLE_SCHEMA").unwrap_or("dbo");
        let table: &str = row.get("TABLE_NAME").unwrap_or("");
        let constraint: &str = row.get("CONSTRAINT_NAME").unwrap_or("");
        let col: &str = row.get("COLUMN_NAME").unwrap_or("");

        uq_map
            .entry((
                schema.to_string(),
                table.to_string(),
                constraint.to_string(),
            ))
            .or_default()
            .push(col.to_string());
    }

    for ((schema, table, _), cols) in uq_map {
        let key = (schema, table);
        if let Some(table_info) = tables.get_mut(&key) {
            table_info.unique_constraints.push(cols);
        }
    }

    let count = tables.len();

    // 6. Load change tracking status
    let ct_rows = client
        .execute(
            "SELECT s.name AS schema_name, t.name AS table_name \
             FROM sys.change_tracking_tables ct \
             JOIN sys.tables t ON ct.object_id = t.object_id \
             JOIN sys.schemas s ON t.schema_id = s.schema_id",
            &[],
        )
        .await;
    // Change tracking may not be enabled on the database — that's fine, just skip
    if let Ok(ct_stream) = ct_rows {
        if let Ok(ct_result) = ct_stream.into_first_result().await {
            for row in &ct_result {
                let schema: &str = row.get("schema_name").unwrap_or("dbo");
                let table: &str = row.get("table_name").unwrap_or("");
                let key = (schema.to_string(), table.to_string());
                if let Some(table_info) = tables.get_mut(&key) {
                    table_info.change_tracking_enabled = true;
                }
            }
        }
    }

    tracing::info!("Schema loaded: {} tables/views", count);

    Ok(SchemaCache {
        tables,
        reverse_fks,
    })
}
