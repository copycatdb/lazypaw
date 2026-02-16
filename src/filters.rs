//! PostgREST-compatible filter parser.
//!
//! Parses query parameters like `?col=eq.value`, `?col=gt.5`,
//! `?or=(col1.eq.a,col2.gt.5)` into a structured filter tree.

use crate::error::Error;

/// A single filter condition.
#[derive(Debug, Clone)]
pub struct Filter {
    pub column: String,
    pub operator: FilterOp,
    pub value: FilterValue,
    pub negated: bool,
}

/// Filter operators.
#[derive(Debug, Clone)]
pub enum FilterOp {
    Eq,
    Gt,
    Gte,
    Lt,
    Lte,
    Neq,
    Like,
    Ilike,
    In,
    Is,
    Fts, // full text search (basic)
}

/// Filter value types.
#[derive(Debug, Clone)]
pub enum FilterValue {
    Single(String),
    List(Vec<String>),
}

/// A group of filters combined with AND or OR.
#[derive(Debug, Clone)]
pub enum FilterNode {
    Condition(Filter),
    And(Vec<FilterNode>),
    Or(Vec<FilterNode>),
}

/// Parse a PostgREST filter expression string (e.g., "eq.value", "in.(a,b,c)")
/// into a Filter for the given column.
pub fn parse_filter(column: &str, expr: &str) -> Result<Filter, Error> {
    let (negated, rest) = if let Some(stripped) = expr.strip_prefix("not.") {
        (true, stripped)
    } else {
        (false, expr)
    };

    // Parse operator and value
    if let Some(value) = rest.strip_prefix("eq.") {
        Ok(Filter {
            column: column.to_string(),
            operator: FilterOp::Eq,
            value: FilterValue::Single(value.to_string()),
            negated,
        })
    } else if let Some(value) = rest.strip_prefix("neq.") {
        Ok(Filter {
            column: column.to_string(),
            operator: FilterOp::Neq,
            value: FilterValue::Single(value.to_string()),
            negated,
        })
    } else if let Some(value) = rest.strip_prefix("gt.") {
        Ok(Filter {
            column: column.to_string(),
            operator: FilterOp::Gt,
            value: FilterValue::Single(value.to_string()),
            negated,
        })
    } else if let Some(value) = rest.strip_prefix("gte.") {
        Ok(Filter {
            column: column.to_string(),
            operator: FilterOp::Gte,
            value: FilterValue::Single(value.to_string()),
            negated,
        })
    } else if let Some(value) = rest.strip_prefix("lt.") {
        Ok(Filter {
            column: column.to_string(),
            operator: FilterOp::Lt,
            value: FilterValue::Single(value.to_string()),
            negated,
        })
    } else if let Some(value) = rest.strip_prefix("lte.") {
        Ok(Filter {
            column: column.to_string(),
            operator: FilterOp::Lte,
            value: FilterValue::Single(value.to_string()),
            negated,
        })
    } else if let Some(value) = rest.strip_prefix("like.") {
        Ok(Filter {
            column: column.to_string(),
            operator: FilterOp::Like,
            value: FilterValue::Single(value.replace('*', "%")),
            negated,
        })
    } else if let Some(value) = rest.strip_prefix("ilike.") {
        Ok(Filter {
            column: column.to_string(),
            operator: FilterOp::Ilike,
            value: FilterValue::Single(value.replace('*', "%")),
            negated,
        })
    } else if let Some(value) = rest.strip_prefix("in.") {
        let items = parse_list(value)?;
        Ok(Filter {
            column: column.to_string(),
            operator: FilterOp::In,
            value: FilterValue::List(items),
            negated,
        })
    } else if let Some(value) = rest.strip_prefix("is.") {
        Ok(Filter {
            column: column.to_string(),
            operator: FilterOp::Is,
            value: FilterValue::Single(value.to_string()),
            negated,
        })
    } else if let Some(value) = rest.strip_prefix("fts.") {
        Ok(Filter {
            column: column.to_string(),
            operator: FilterOp::Fts,
            value: FilterValue::Single(value.to_string()),
            negated,
        })
    } else {
        Err(Error::BadRequest(format!(
            "Unknown filter expression: {}",
            expr
        )))
    }
}

/// Parse a parenthesized list: "(a,b,c)" -> vec!["a", "b", "c"]
fn parse_list(s: &str) -> Result<Vec<String>, Error> {
    let s = s.trim();
    let inner = if s.starts_with('(') && s.ends_with(')') {
        &s[1..s.len() - 1]
    } else {
        s
    };

    Ok(inner
        .split(',')
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect())
}

/// Parse an OR/AND group expression: "(col1.eq.a,col2.gt.5)"
/// Supports nested and/or: "(status.eq.waiting,and(score.gt.50,name.like.*cat*))"
pub fn parse_logic_group(expr: &str) -> Result<Vec<FilterNode>, Error> {
    let s = expr.trim();
    let inner = if s.starts_with('(') && s.ends_with(')') {
        &s[1..s.len() - 1]
    } else {
        s
    };

    // Split on commas, but respect nested parens
    let parts = split_respecting_parens(inner);
    let mut nodes = Vec::new();

    for part in parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        // Check for nested or(...) / and(...)
        if let Some(inner_expr) = part.strip_prefix("or") {
            if inner_expr.starts_with('(') && inner_expr.ends_with(')') {
                let children = parse_logic_group(inner_expr)?;
                nodes.push(FilterNode::Or(children));
                continue;
            }
        }
        if let Some(inner_expr) = part.strip_prefix("and") {
            if inner_expr.starts_with('(') && inner_expr.ends_with(')') {
                let children = parse_logic_group(inner_expr)?;
                nodes.push(FilterNode::And(children));
                continue;
            }
        }
        // Find first dot that separates column from operator
        if let Some(dot_pos) = part.find('.') {
            let col = &part[..dot_pos];
            let rest = &part[dot_pos + 1..];
            let filter = parse_filter(col, rest)?;
            nodes.push(FilterNode::Condition(filter));
        } else {
            return Err(Error::BadRequest(format!(
                "Invalid filter in group: {}",
                part
            )));
        }
    }

    Ok(nodes)
}

/// Split a string by commas, but don't split inside parentheses.
fn split_respecting_parens(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0;

    for ch in s.chars() {
        match ch {
            '(' => {
                depth += 1;
                current.push(ch);
            }
            ')' => {
                depth -= 1;
                current.push(ch);
            }
            ',' if depth == 0 => {
                parts.push(current.clone());
                current.clear();
            }
            _ => {
                current.push(ch);
            }
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_eq() {
        let f = parse_filter("name", "eq.alice").unwrap();
        assert_eq!(f.column, "name");
        assert!(!f.negated);
        assert!(matches!(f.operator, FilterOp::Eq));
        assert!(matches!(f.value, FilterValue::Single(ref v) if v == "alice"));
    }

    #[test]
    fn test_parse_not_eq() {
        let f = parse_filter("name", "not.eq.alice").unwrap();
        assert!(f.negated);
        assert!(matches!(f.operator, FilterOp::Eq));
    }

    #[test]
    fn test_parse_in() {
        let f = parse_filter("id", "in.(1,2,3)").unwrap();
        assert!(matches!(f.operator, FilterOp::In));
        if let FilterValue::List(items) = &f.value {
            assert_eq!(items, &["1", "2", "3"]);
        } else {
            panic!("Expected list value");
        }
    }

    #[test]
    fn test_parse_like() {
        let f = parse_filter("name", "like.*alice*").unwrap();
        assert!(matches!(f.operator, FilterOp::Like));
        assert!(matches!(f.value, FilterValue::Single(ref v) if v == "%alice%"));
    }

    #[test]
    fn test_parse_is_null() {
        let f = parse_filter("deleted_at", "is.null").unwrap();
        assert!(matches!(f.operator, FilterOp::Is));
        assert!(matches!(f.value, FilterValue::Single(ref v) if v == "null"));
    }

    #[test]
    fn test_logic_group() {
        let nodes = parse_logic_group("(name.eq.alice,age.gt.25)").unwrap();
        assert_eq!(nodes.len(), 2);
        match &nodes[0] {
            FilterNode::Condition(f) => assert_eq!(f.column, "name"),
            _ => panic!("Expected Condition"),
        }
        match &nodes[1] {
            FilterNode::Condition(f) => assert_eq!(f.column, "age"),
            _ => panic!("Expected Condition"),
        }
    }

    #[test]
    fn test_nested_logic_group() {
        let nodes =
            parse_logic_group("(status.eq.waiting,and(score.gt.50,name.like.*cat*))").unwrap();
        assert_eq!(nodes.len(), 2);
        match &nodes[0] {
            FilterNode::Condition(f) => assert_eq!(f.column, "status"),
            _ => panic!("Expected Condition"),
        }
        match &nodes[1] {
            FilterNode::And(children) => assert_eq!(children.len(), 2),
            _ => panic!("Expected And"),
        }
    }
}
