//! SQL Server type → JSON/Arrow type mapping.

use claw::SqlValue;
use serde_json::Value as JsonValue;

/// Map a SQL Server INFORMATION_SCHEMA DATA_TYPE string to an OpenAPI type.
pub fn sql_type_to_openapi(data_type: &str) -> (&'static str, &'static str) {
    match data_type.to_lowercase().as_str() {
        "bit" => ("boolean", ""),
        "tinyint" => ("integer", "int32"),
        "smallint" => ("integer", "int32"),
        "int" => ("integer", "int32"),
        "bigint" => ("integer", "int64"),
        "float" | "real" => ("number", "double"),
        "decimal" | "numeric" | "money" | "smallmoney" => ("number", "decimal"),
        "char" | "varchar" | "nchar" | "nvarchar" | "text" | "ntext" => ("string", ""),
        "date" => ("string", "date"),
        "time" => ("string", "time"),
        "datetime" | "datetime2" | "smalldatetime" => ("string", "date-time"),
        "datetimeoffset" => ("string", "date-time"),
        "uniqueidentifier" => ("string", "uuid"),
        "binary" | "varbinary" | "image" => ("string", "byte"),
        "xml" => ("string", "xml"),
        "geography" | "geometry" => ("string", ""),
        _ => ("string", ""),
    }
}

/// Convert a claw SqlValue to a serde_json Value.
pub fn sql_value_to_json(val: &SqlValue<'_>) -> JsonValue {
    match val {
        SqlValue::U8(Some(v)) => JsonValue::Number((*v as u64).into()),
        SqlValue::U8(None) => JsonValue::Null,
        SqlValue::I16(Some(v)) => JsonValue::Number((*v as i64).into()),
        SqlValue::I16(None) => JsonValue::Null,
        SqlValue::I32(Some(v)) => JsonValue::Number((*v as i64).into()),
        SqlValue::I32(None) => JsonValue::Null,
        SqlValue::I64(Some(v)) => JsonValue::Number(serde_json::Number::from(*v)),
        SqlValue::I64(None) => JsonValue::Null,
        SqlValue::F32(Some(v)) => serde_json::Number::from_f64(*v as f64)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        SqlValue::F32(None) => JsonValue::Null,
        SqlValue::F64(Some(v)) => serde_json::Number::from_f64(*v)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        SqlValue::F64(None) => JsonValue::Null,
        SqlValue::Bit(Some(v)) => JsonValue::Bool(*v),
        SqlValue::Bit(None) => JsonValue::Null,
        SqlValue::String(Some(v)) => JsonValue::String(v.to_string()),
        SqlValue::String(None) => JsonValue::Null,
        SqlValue::Guid(Some(v)) => JsonValue::String(v.to_string()),
        SqlValue::Guid(None) => JsonValue::Null,
        SqlValue::Binary(Some(v)) => {
            use base64::Engine;
            JsonValue::String(base64::engine::general_purpose::STANDARD.encode(v.as_ref()))
        }
        SqlValue::Binary(None) => JsonValue::Null,
        SqlValue::Numeric(Some(v)) => {
            // Render as string to preserve precision
            let raw = v.value();
            let scale = v.scale();
            if scale == 0 {
                if let Some(n) = serde_json::Number::from_f64(raw as f64) {
                    return JsonValue::Number(n);
                }
                JsonValue::String(raw.to_string())
            } else {
                let s = format_decimal(raw, scale);
                // Try to parse as f64 for JSON number
                if let Ok(f) = s.parse::<f64>() {
                    if let Some(n) = serde_json::Number::from_f64(f) {
                        return JsonValue::Number(n);
                    }
                }
                JsonValue::String(s)
            }
        }
        SqlValue::Numeric(None) => JsonValue::Null,
        SqlValue::Xml(Some(v)) => JsonValue::String(format!("{}", v)),
        SqlValue::Xml(None) => JsonValue::Null,
        SqlValue::DateTime(Some(dt)) => {
            let base = chrono::NaiveDate::from_ymd_opt(1900, 1, 1).unwrap();
            let date = base + chrono::Duration::days(dt.days() as i64);
            let ticks = dt.seconds_fragments() as i64;
            let total_ms = ticks * 1000 / 300;
            let secs = (total_ms / 1000) as u32;
            let nanos = ((total_ms % 1000) * 1_000_000) as u32;
            let time = chrono::NaiveTime::from_num_seconds_from_midnight_opt(secs, nanos)
                .unwrap_or_default();
            let ndt = chrono::NaiveDateTime::new(date, time);
            JsonValue::String(format!("{}Z", ndt.format("%Y-%m-%dT%H:%M:%S%.3f")))
        }
        SqlValue::DateTime(None) => JsonValue::Null,
        SqlValue::SmallDateTime(Some(dt)) => {
            let base = chrono::NaiveDate::from_ymd_opt(1900, 1, 1).unwrap();
            let date = base + chrono::Duration::days(dt.days() as i64);
            let mins = dt.seconds_fragments() as u32;
            let time = chrono::NaiveTime::from_num_seconds_from_midnight_opt(mins * 60, 0)
                .unwrap_or_default();
            let ndt = chrono::NaiveDateTime::new(date, time);
            JsonValue::String(ndt.format("%Y-%m-%dT%H:%M:%S").to_string())
        }
        SqlValue::SmallDateTime(None) => JsonValue::Null,
        SqlValue::Date(Some(d)) => {
            let base = chrono::NaiveDate::from_ymd_opt(1, 1, 1).unwrap();
            let date = base + chrono::Duration::days(d.days() as i64);
            JsonValue::String(date.format("%Y-%m-%d").to_string())
        }
        SqlValue::Date(None) => JsonValue::Null,
        SqlValue::Time(Some(t)) => {
            let nanos = t.increments() * 10u64.pow(9u32.saturating_sub(t.scale() as u32));
            let secs = (nanos / 1_000_000_000) as u32;
            let remaining = (nanos % 1_000_000_000) as u32;
            let time = chrono::NaiveTime::from_num_seconds_from_midnight_opt(secs, remaining)
                .unwrap_or_default();
            JsonValue::String(time.format("%H:%M:%S%.f").to_string())
        }
        SqlValue::Time(None) => JsonValue::Null,
        SqlValue::DateTime2(Some(dt)) => {
            let base = chrono::NaiveDate::from_ymd_opt(1, 1, 1).unwrap();
            let date = base + chrono::Duration::days(dt.date().days() as i64);
            let t = dt.time();
            let nanos = t.increments() * 10u64.pow(9u32.saturating_sub(t.scale() as u32));
            let secs = (nanos / 1_000_000_000) as u32;
            let remaining = (nanos % 1_000_000_000) as u32;
            let time = chrono::NaiveTime::from_num_seconds_from_midnight_opt(secs, remaining)
                .unwrap_or_default();
            let ndt = chrono::NaiveDateTime::new(date, time);
            JsonValue::String(format!("{}Z", ndt.format("%Y-%m-%dT%H:%M:%S%.3f")))
        }
        SqlValue::DateTime2(None) => JsonValue::Null,
        SqlValue::DateTimeOffset(Some(dto)) => {
            let dt = dto.datetime2();
            let base = chrono::NaiveDate::from_ymd_opt(1, 1, 1).unwrap();
            let date = base + chrono::Duration::days(dt.date().days() as i64);
            let t = dt.time();
            let nanos = t.increments() * 10u64.pow(9u32.saturating_sub(t.scale() as u32));
            let secs = (nanos / 1_000_000_000) as u32;
            let remaining = (nanos % 1_000_000_000) as u32;
            let time = chrono::NaiveTime::from_num_seconds_from_midnight_opt(secs, remaining)
                .unwrap_or_default();
            let ndt = chrono::NaiveDateTime::new(date, time);
            let offset_mins = dto.offset();
            if offset_mins == 0 {
                JsonValue::String(format!("{}Z", ndt.format("%Y-%m-%dT%H:%M:%S%.3f")))
            } else {
                let sign = if offset_mins >= 0 { "+" } else { "-" };
                let abs_mins = offset_mins.unsigned_abs();
                let oh = abs_mins / 60;
                let om = abs_mins % 60;
                JsonValue::String(format!(
                    "{}{}{:02}:{:02}",
                    ndt.format("%Y-%m-%dT%H:%M:%S%.3f"),
                    sign,
                    oh,
                    om
                ))
            }
        }
        SqlValue::DateTimeOffset(None) => JsonValue::Null,
    }
}

/// Convert a Row into a JSON object.
pub fn row_to_json(row: &claw::Row) -> serde_json::Map<String, JsonValue> {
    let mut obj = serde_json::Map::new();
    for (col, val) in row.cells() {
        obj.insert(col.name().to_string(), sql_value_to_json(val));
    }
    obj
}

/// Format a decimal i128 value with given scale.
fn format_decimal(value: i128, scale: u8) -> String {
    if scale == 0 {
        return value.to_string();
    }
    let is_negative = value < 0;
    let abs_val = value.unsigned_abs();
    let divisor = 10u128.pow(scale as u32);
    let integer_part = abs_val / divisor;
    let fractional_part = abs_val % divisor;
    let sign = if is_negative { "-" } else { "" };
    format!(
        "{}{}.{:0>width$}",
        sign,
        integer_part,
        fractional_part,
        width = scale as usize
    )
}
