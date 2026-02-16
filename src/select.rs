//! Select & embedding parser.
//!
//! Parses PostgREST-style select expressions:
//! - `?select=col1,col2` — column selection
//! - `?select=*,orders(*)` — embed related table via FK
//! - `?select=*,orders!fk_name(id,amount)` — disambiguate FK + column selection
//! - `?select=*,orders(items(*))` — nested embedding

use crate::error::Error;

/// A parsed select expression node.
#[derive(Debug, Clone)]
pub enum SelectNode {
    /// Select all columns: `*`
    Star,
    /// Select a specific column
    Column(String),
    /// Embed a related table with optional FK hint and sub-select
    Embed(EmbedSelect),
}

/// An embedding specification.
#[derive(Debug, Clone)]
pub struct EmbedSelect {
    /// The name of the related table to embed
    pub name: String,
    /// Optional FK constraint name hint (from `!fk_name`)
    pub fk_hint: Option<String>,
    /// Sub-select within the embedded table
    pub columns: Vec<SelectNode>,
}

/// Parse a full select expression string.
pub fn parse_select(input: &str) -> Result<Vec<SelectNode>, Error> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(vec![SelectNode::Star]);
    }

    let tokens = split_top_level(input);
    let mut nodes = Vec::new();

    for token in tokens {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        nodes.push(parse_select_token(token)?);
    }

    Ok(nodes)
}

/// Parse a single select token.
fn parse_select_token(token: &str) -> Result<SelectNode, Error> {
    if token == "*" {
        return Ok(SelectNode::Star);
    }

    // Check for embedding: name(...) or name!fk_hint(...)
    if let Some(paren_start) = token.find('(') {
        if !token.ends_with(')') {
            return Err(Error::BadRequest(format!(
                "Unmatched parenthesis in select: {}",
                token
            )));
        }

        let prefix = &token[..paren_start];
        let inner = &token[paren_start + 1..token.len() - 1];

        // Check for FK hint: name!fk_name
        let (name, fk_hint) = if let Some(bang_pos) = prefix.find('!') {
            (
                prefix[..bang_pos].to_string(),
                Some(prefix[bang_pos + 1..].to_string()),
            )
        } else {
            (prefix.to_string(), None)
        };

        // Parse inner columns recursively
        let columns = parse_select(inner)?;

        Ok(SelectNode::Embed(EmbedSelect {
            name,
            fk_hint,
            columns,
        }))
    } else {
        // Check for rename: alias:column (not implementing rename for now, just parse column)
        let col = if let Some(colon_pos) = token.find(':') {
            token[colon_pos + 1..].to_string()
        } else {
            token.to_string()
        };
        Ok(SelectNode::Column(col))
    }
}

/// Split a string by top-level commas (not inside parentheses).
fn split_top_level(s: &str) -> Vec<String> {
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

/// Extract the list of plain column names from a select expression
/// (ignoring embeds and stars).
pub fn select_columns(nodes: &[SelectNode]) -> Vec<&str> {
    let mut cols = Vec::new();
    for node in nodes {
        match node {
            SelectNode::Column(name) => cols.push(name.as_str()),
            SelectNode::Star | SelectNode::Embed(_) => {}
        }
    }
    cols
}

/// Check if the select has a star.
pub fn has_star(nodes: &[SelectNode]) -> bool {
    nodes.iter().any(|n| matches!(n, SelectNode::Star))
}

/// Extract embed specifications from the select.
pub fn select_embeds(nodes: &[SelectNode]) -> Vec<&EmbedSelect> {
    nodes
        .iter()
        .filter_map(|n| match n {
            SelectNode::Embed(e) => Some(e),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_columns() {
        let nodes = parse_select("id,name,email").unwrap();
        assert_eq!(nodes.len(), 3);
        assert!(matches!(&nodes[0], SelectNode::Column(c) if c == "id"));
        assert!(matches!(&nodes[1], SelectNode::Column(c) if c == "name"));
        assert!(matches!(&nodes[2], SelectNode::Column(c) if c == "email"));
    }

    #[test]
    fn test_star_with_embed() {
        let nodes = parse_select("*,orders(*)").unwrap();
        assert_eq!(nodes.len(), 2);
        assert!(matches!(&nodes[0], SelectNode::Star));
        if let SelectNode::Embed(e) = &nodes[1] {
            assert_eq!(e.name, "orders");
            assert!(e.fk_hint.is_none());
            assert_eq!(e.columns.len(), 1);
            assert!(matches!(&e.columns[0], SelectNode::Star));
        } else {
            panic!("Expected embed");
        }
    }

    #[test]
    fn test_embed_with_fk_hint() {
        let nodes = parse_select("*,orders!fk_customer(id,amount)").unwrap();
        assert_eq!(nodes.len(), 2);
        if let SelectNode::Embed(e) = &nodes[1] {
            assert_eq!(e.name, "orders");
            assert_eq!(e.fk_hint.as_deref(), Some("fk_customer"));
            assert_eq!(e.columns.len(), 2);
        } else {
            panic!("Expected embed");
        }
    }

    #[test]
    fn test_nested_embed() {
        let nodes = parse_select("*,orders(items(*))").unwrap();
        assert_eq!(nodes.len(), 2);
        if let SelectNode::Embed(e) = &nodes[1] {
            assert_eq!(e.name, "orders");
            assert_eq!(e.columns.len(), 1);
            if let SelectNode::Embed(inner) = &e.columns[0] {
                assert_eq!(inner.name, "items");
            } else {
                panic!("Expected nested embed");
            }
        } else {
            panic!("Expected embed");
        }
    }
}
