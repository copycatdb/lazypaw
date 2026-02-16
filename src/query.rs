//! SQL query builder from URL parameters.
//!
//! Builds parameterized SQL queries for SELECT, INSERT, UPDATE, DELETE
//! operations based on parsed filters, select, ordering, and pagination.

use crate::error::Error;
use crate::filters::{Filter, FilterNode, FilterOp, FilterValue};
use crate::schema::TableInfo;
use crate::select::{self, SelectNode};

/// A built SQL query with parameterized values.
#[derive(Debug)]
pub struct BuiltQuery {
    pub sql: String,
    pub params: Vec<String>,
}

/// Ordering specification.
#[derive(Debug, Clone)]
pub struct OrderSpec {
    pub column: String,
    pub direction: OrderDir,
    pub nulls: Option<NullsOrder>,
}

#[derive(Debug, Clone)]
pub enum OrderDir {
    Asc,
    Desc,
}

#[derive(Debug, Clone)]
pub enum NullsOrder {
    First,
    Last,
}

/// Parse order query param: "name.asc,age.desc.nullsfirst"
pub fn parse_order(order_str: &str) -> Result<Vec<OrderSpec>, Error> {
    let mut specs = Vec::new();
    for part in order_str.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let segments: Vec<&str> = part.split('.').collect();
        if segments.is_empty() {
            continue;
        }

        let column = segments[0].to_string();
        let direction = if segments.len() > 1 {
            match segments[1].to_lowercase().as_str() {
                "desc" => OrderDir::Desc,
                _ => OrderDir::Asc,
            }
        } else {
            OrderDir::Asc
        };

        let nulls = if segments.len() > 2 {
            match segments[2].to_lowercase().as_str() {
                "nullsfirst" => Some(NullsOrder::First),
                "nullslast" => Some(NullsOrder::Last),
                _ => None,
            }
        } else {
            None
        };

        specs.push(OrderSpec {
            column,
            direction,
            nulls,
        });
    }
    Ok(specs)
}

/// Build a SELECT query from filters, select, ordering, and pagination.
pub fn build_select(
    table: &TableInfo,
    select_nodes: &[SelectNode],
    filters: &[FilterNode],
    order: &[OrderSpec],
    limit: Option<i64>,
    offset: Option<i64>,
    count_only: bool,
) -> Result<BuiltQuery, Error> {
    let mut params: Vec<String> = Vec::new();

    // Build column list
    let columns = if count_only {
        "COUNT(*) AS [count]".to_string()
    } else {
        build_column_list(table, select_nodes)
    };

    let mut sql = format!("SELECT {} FROM {}", columns, table.full_name());

    // WHERE clause
    if !filters.is_empty() {
        let where_clause = build_where_clause(filters, &mut params)?;
        if !where_clause.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_clause);
        }
    }

    if count_only {
        return Ok(BuiltQuery { sql, params });
    }

    // ORDER BY
    if !order.is_empty() {
        sql.push_str(" ORDER BY ");
        let order_parts: Vec<String> = order
            .iter()
            .map(|o| {
                let dir = match o.direction {
                    OrderDir::Asc => "ASC",
                    OrderDir::Desc => "DESC",
                };
                let nulls = match &o.nulls {
                    Some(NullsOrder::First) => {
                        format!(
                            "CASE WHEN [{}] IS NULL THEN 0 ELSE 1 END, ",
                            escape_ident(&o.column)
                        )
                    }
                    Some(NullsOrder::Last) => {
                        format!(
                            "CASE WHEN [{}] IS NULL THEN 1 ELSE 0 END, ",
                            escape_ident(&o.column)
                        )
                    }
                    None => String::new(),
                };
                format!("{}[{}] {}", nulls, escape_ident(&o.column), dir)
            })
            .collect();
        sql.push_str(&order_parts.join(", "));
    } else if limit.is_some() || offset.is_some() {
        // ORDER BY is required for OFFSET/FETCH
        if !table.primary_key.is_empty() {
            let pk_order: Vec<String> = table
                .primary_key
                .iter()
                .map(|c| format!("[{}] ASC", escape_ident(c)))
                .collect();
            sql.push_str(" ORDER BY ");
            sql.push_str(&pk_order.join(", "));
        } else {
            sql.push_str(" ORDER BY (SELECT NULL)");
        }
    }

    // OFFSET/FETCH for pagination
    if let Some(off) = offset {
        sql.push_str(&format!(" OFFSET {} ROWS", off));
        if let Some(lim) = limit {
            sql.push_str(&format!(" FETCH NEXT {} ROWS ONLY", lim));
        }
    } else if let Some(lim) = limit {
        sql.push_str(&format!(" OFFSET 0 ROWS FETCH NEXT {} ROWS ONLY", lim));
    }

    Ok(BuiltQuery { sql, params })
}

/// Build an INSERT query.
pub fn build_insert(
    table: &TableInfo,
    columns: &[String],
    value_count: usize,
) -> Result<BuiltQuery, Error> {
    if columns.is_empty() {
        return Err(Error::BadRequest("No columns to insert".to_string()));
    }

    let col_list: Vec<String> = columns
        .iter()
        .map(|c| format!("[{}]", escape_ident(c)))
        .collect();

    let mut param_idx = 1;
    let mut all_value_groups = Vec::new();

    for _ in 0..value_count {
        let group: Vec<String> = columns
            .iter()
            .map(|_| {
                let p = format!("@P{}", param_idx);
                param_idx += 1;
                p
            })
            .collect();
        all_value_groups.push(format!("({})", group.join(", ")));
    }

    // Build OUTPUT clause for all columns
    let output_cols: Vec<String> = table
        .columns
        .iter()
        .map(|c| format!("inserted.[{}]", escape_ident(&c.name)))
        .collect();

    let sql = format!(
        "INSERT INTO {} ({}) OUTPUT {} VALUES {}",
        table.full_name(),
        col_list.join(", "),
        output_cols.join(", "),
        all_value_groups.join(", ")
    );

    Ok(BuiltQuery {
        sql,
        params: Vec::new(),
    })
}

/// Build a MERGE (upsert) query.
pub fn build_upsert(
    table: &TableInfo,
    columns: &[String],
    _value_count: usize,
) -> Result<BuiltQuery, Error> {
    if columns.is_empty() {
        return Err(Error::BadRequest("No columns to upsert".to_string()));
    }

    // Need PK or unique constraint for merge match
    let match_cols = if !table.primary_key.is_empty() {
        &table.primary_key
    } else if let Some(uq) = table.unique_constraints.first() {
        uq
    } else {
        return Err(Error::BadRequest(
            "Table has no primary key or unique constraint for upsert".to_string(),
        ));
    };

    let col_list: Vec<String> = columns
        .iter()
        .map(|c| format!("[{}]", escape_ident(c)))
        .collect();

    let source_cols: Vec<String> = columns
        .iter()
        .enumerate()
        .map(|(i, _)| format!("@P{}", i + 1))
        .collect();

    let on_clause: Vec<String> = match_cols
        .iter()
        .map(|c| {
            format!(
                "target.[{}] = source.[{}]",
                escape_ident(c),
                escape_ident(c)
            )
        })
        .collect();

    let update_cols: Vec<String> = columns
        .iter()
        .filter(|c| !match_cols.iter().any(|mc| mc.eq_ignore_ascii_case(c)))
        .map(|c| {
            format!(
                "target.[{}] = source.[{}]",
                escape_ident(c),
                escape_ident(c)
            )
        })
        .collect();

    let output_cols: Vec<String> = table
        .columns
        .iter()
        .map(|c| format!("inserted.[{}]", escape_ident(&c.name)))
        .collect();

    let mut sql = format!(
        "MERGE {} AS target USING (SELECT {}) AS source ({}) ON {} ",
        table.full_name(),
        source_cols
            .iter()
            .zip(columns.iter())
            .map(|(p, c)| format!("{} AS [{}]", p, escape_ident(c)))
            .collect::<Vec<_>>()
            .join(", "),
        col_list.join(", "),
        on_clause.join(" AND ")
    );

    if !update_cols.is_empty() {
        sql.push_str(&format!(
            "WHEN MATCHED THEN UPDATE SET {} ",
            update_cols.join(", ")
        ));
    }

    sql.push_str(&format!(
        "WHEN NOT MATCHED THEN INSERT ({}) VALUES ({}) OUTPUT {};",
        col_list.join(", "),
        columns
            .iter()
            .map(|c| format!("source.[{}]", escape_ident(c)))
            .collect::<Vec<_>>()
            .join(", "),
        output_cols.join(", ")
    ));

    Ok(BuiltQuery {
        sql,
        params: Vec::new(),
    })
}

/// Build an UPDATE query with filters.
pub fn build_update(
    table: &TableInfo,
    columns: &[String],
    filters: &[FilterNode],
) -> Result<BuiltQuery, Error> {
    if columns.is_empty() {
        return Err(Error::BadRequest("No columns to update".to_string()));
    }

    let mut params: Vec<String> = Vec::new();

    let set_clauses: Vec<String> = columns
        .iter()
        .enumerate()
        .map(|(i, c)| format!("[{}] = @P{}", escape_ident(c), i + 1))
        .collect();

    let param_offset = columns.len();

    let output_cols: Vec<String> = table
        .columns
        .iter()
        .map(|c| format!("inserted.[{}]", escape_ident(&c.name)))
        .collect();

    let mut sql = format!(
        "UPDATE {} SET {} OUTPUT {}",
        table.full_name(),
        set_clauses.join(", "),
        output_cols.join(", ")
    );

    if !filters.is_empty() {
        let where_clause = build_where_clause_with_offset(filters, &mut params, param_offset)?;
        if !where_clause.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_clause);
        }
    }

    Ok(BuiltQuery { sql, params })
}

/// Build a DELETE query with filters.
pub fn build_delete(table: &TableInfo, filters: &[FilterNode]) -> Result<BuiltQuery, Error> {
    let mut params: Vec<String> = Vec::new();

    let output_cols: Vec<String> = table
        .columns
        .iter()
        .map(|c| format!("deleted.[{}]", escape_ident(&c.name)))
        .collect();

    let mut sql = format!(
        "DELETE FROM {} OUTPUT {}",
        table.full_name(),
        output_cols.join(", ")
    );

    if !filters.is_empty() {
        let where_clause = build_where_clause(filters, &mut params)?;
        if !where_clause.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_clause);
        }
    }

    Ok(BuiltQuery { sql, params })
}

/// Build the column list for SELECT from select nodes.
fn build_column_list(table: &TableInfo, nodes: &[SelectNode]) -> String {
    if nodes.is_empty() || select::has_star(nodes) {
        // Select all columns from the table (excluding embeds which are handled separately)
        let explicit_cols = select::select_columns(nodes);
        if explicit_cols.is_empty() {
            return table
                .columns
                .iter()
                .map(|c| format!("[{}]", escape_ident(&c.name)))
                .collect::<Vec<_>>()
                .join(", ");
        }
        // Star + explicit columns
        let mut cols: Vec<String> = table
            .columns
            .iter()
            .map(|c| format!("[{}]", escape_ident(&c.name)))
            .collect();
        for col in explicit_cols {
            if !table.columns.iter().any(|c| c.name.eq_ignore_ascii_case(col)) {
                cols.push(format!("[{}]", escape_ident(col)));
            }
        }
        cols.join(", ")
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

/// Build WHERE clause from filter nodes.
fn build_where_clause(
    filters: &[FilterNode],
    params: &mut Vec<String>,
) -> Result<String, Error> {
    build_where_clause_with_offset(filters, params, 0)
}

/// Build WHERE clause from filter nodes with a parameter index offset.
fn build_where_clause_with_offset(
    filters: &[FilterNode],
    params: &mut Vec<String>,
    offset: usize,
) -> Result<String, Error> {
    let mut parts = Vec::new();

    for node in filters {
        let clause = build_filter_node(node, params, offset)?;
        if !clause.is_empty() {
            parts.push(clause);
        }
    }

    Ok(parts.join(" AND "))
}

/// Build SQL from a single filter node.
fn build_filter_node(
    node: &FilterNode,
    params: &mut Vec<String>,
    offset: usize,
) -> Result<String, Error> {
    match node {
        FilterNode::Condition(filter) => build_single_filter(filter, params, offset),
        FilterNode::And(nodes) => {
            let parts: Result<Vec<String>, _> = nodes
                .iter()
                .map(|n| build_filter_node(n, params, offset))
                .collect();
            let parts = parts?;
            let non_empty: Vec<_> = parts.into_iter().filter(|p| !p.is_empty()).collect();
            if non_empty.is_empty() {
                Ok(String::new())
            } else {
                Ok(format!("({})", non_empty.join(" AND ")))
            }
        }
        FilterNode::Or(nodes) => {
            let parts: Result<Vec<String>, _> = nodes
                .iter()
                .map(|n| build_filter_node(n, params, offset))
                .collect();
            let parts = parts?;
            let non_empty: Vec<_> = parts.into_iter().filter(|p| !p.is_empty()).collect();
            if non_empty.is_empty() {
                Ok(String::new())
            } else {
                Ok(format!("({})", non_empty.join(" OR ")))
            }
        }
    }
}

/// Build SQL for a single filter condition.
fn build_single_filter(
    filter: &Filter,
    params: &mut Vec<String>,
    offset: usize,
) -> Result<String, Error> {
    let col = format!("[{}]", escape_ident(&filter.column));
    let not_prefix = if filter.negated { "NOT " } else { "" };

    match &filter.operator {
        FilterOp::Eq => {
            params.push(filter_value_single(&filter.value)?);
            let idx = params.len() + offset;
            Ok(format!("{}({} = @P{})", not_prefix, col, idx))
        }
        FilterOp::Neq => {
            params.push(filter_value_single(&filter.value)?);
            let idx = params.len() + offset;
            Ok(format!("{}({} <> @P{})", not_prefix, col, idx))
        }
        FilterOp::Gt => {
            params.push(filter_value_single(&filter.value)?);
            let idx = params.len() + offset;
            Ok(format!("{}({} > @P{})", not_prefix, col, idx))
        }
        FilterOp::Gte => {
            params.push(filter_value_single(&filter.value)?);
            let idx = params.len() + offset;
            Ok(format!("{}({} >= @P{})", not_prefix, col, idx))
        }
        FilterOp::Lt => {
            params.push(filter_value_single(&filter.value)?);
            let idx = params.len() + offset;
            Ok(format!("{}({} < @P{})", not_prefix, col, idx))
        }
        FilterOp::Lte => {
            params.push(filter_value_single(&filter.value)?);
            let idx = params.len() + offset;
            Ok(format!("{}({} <= @P{})", not_prefix, col, idx))
        }
        FilterOp::Like => {
            params.push(filter_value_single(&filter.value)?);
            let idx = params.len() + offset;
            Ok(format!("{}({} LIKE @P{})", not_prefix, col, idx))
        }
        FilterOp::Ilike => {
            params.push(filter_value_single(&filter.value)?);
            let idx = params.len() + offset;
            // SQL Server LIKE is case-insensitive by default with most collations
            Ok(format!("{}({} LIKE @P{})", not_prefix, col, idx))
        }
        FilterOp::In => {
            if let FilterValue::List(items) = &filter.value {
                let placeholders: Vec<String> = items
                    .iter()
                    .map(|item| {
                        params.push(item.clone());
                        let idx = params.len() + offset;
                        format!("@P{}", idx)
                    })
                    .collect();
                Ok(format!(
                    "{}({} IN ({}))",
                    not_prefix,
                    col,
                    placeholders.join(", ")
                ))
            } else {
                Err(Error::BadRequest("IN requires a list value".to_string()))
            }
        }
        FilterOp::Is => {
            let val = filter_value_single(&filter.value)?;
            match val.to_lowercase().as_str() {
                "null" => {
                    if filter.negated {
                        Ok(format!("{} IS NOT NULL", col))
                    } else {
                        Ok(format!("{} IS NULL", col))
                    }
                }
                "true" => {
                    if filter.negated {
                        Ok(format!("{} <> 1", col))
                    } else {
                        Ok(format!("{} = 1", col))
                    }
                }
                "false" => {
                    if filter.negated {
                        Ok(format!("{} <> 0", col))
                    } else {
                        Ok(format!("{} = 0", col))
                    }
                }
                _ => Err(Error::BadRequest(format!(
                    "IS only supports null, true, false; got: {}",
                    val
                ))),
            }
        }
        FilterOp::Fts => {
            params.push(filter_value_single(&filter.value)?);
            let idx = params.len() + offset;
            Ok(format!(
                "{}CONTAINS({}, @P{})",
                not_prefix, col, idx
            ))
        }
    }
}

/// Extract a single string value from a FilterValue.
fn filter_value_single(val: &FilterValue) -> Result<String, Error> {
    match val {
        FilterValue::Single(s) => Ok(s.clone()),
        FilterValue::List(items) => {
            if items.len() == 1 {
                Ok(items[0].clone())
            } else {
                Err(Error::BadRequest(
                    "Expected single value, got list".to_string(),
                ))
            }
        }
    }
}

/// Escape a SQL Server identifier (remove brackets and re-wrap).
pub fn escape_ident(name: &str) -> String {
    name.replace(']', "]]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_order() {
        let specs = parse_order("name.asc,age.desc.nullsfirst").unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].column, "name");
        assert!(matches!(specs[0].direction, OrderDir::Asc));
        assert_eq!(specs[1].column, "age");
        assert!(matches!(specs[1].direction, OrderDir::Desc));
        assert!(matches!(specs[1].nulls, Some(NullsOrder::First)));
    }
}
