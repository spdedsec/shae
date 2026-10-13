use std::sync::{Arc, RwLock};

use indexmap::IndexMap;

use crate::value::Value;

/// Parse an RFC 4180 compliant CSV string into rows of fields.
pub fn parse_csv_raw(input: &str, delimiter: char) -> Result<Vec<Vec<String>>, String> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut current_field = String::new();
    let mut in_quotes = false;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    // Escaped double quote ("")
                    chars.next();
                    current_field.push('"');
                } else {
                    // End of quoted section
                    in_quotes = false;
                }
            } else {
                current_field.push(ch);
            }
        } else if ch == '"' {
            in_quotes = true;
        } else if ch == delimiter {
            current_row.push(std::mem::take(&mut current_field));
        } else if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            current_row.push(std::mem::take(&mut current_field));
            rows.push(std::mem::take(&mut current_row));
        } else if ch == '\n' {
            current_row.push(std::mem::take(&mut current_field));
            rows.push(std::mem::take(&mut current_row));
        } else {
            current_field.push(ch);
        }
    }

    if in_quotes {
        return Err("Unclosed quoted field in CSV input".to_string());
    }

    // Append last field and row if non-empty
    if !current_field.is_empty() || !current_row.is_empty() {
        current_row.push(current_field);
        rows.push(current_row);
    }

    Ok(rows)
}

fn coerce_value(val_str: &str, typed: bool) -> Value {
    if !typed {
        return Value::String(val_str.to_string());
    }

    let trimmed = val_str.trim();
    if trimmed.is_empty() {
        return Value::String(String::new());
    }
    if trimmed == "true" {
        return Value::Bool(true);
    }
    if trimmed == "false" {
        return Value::Bool(false);
    }
    if trimmed == "null" || trimmed == "nil" {
        return Value::Null;
    }
    if let Ok(n) = trimmed.parse::<i64>() {
        return Value::Int(n);
    }
    if let Ok(f) = trimmed.parse::<f64>() {
        if f.is_finite() && trimmed.contains('.') {
            return Value::Float(f);
        }
    }

    Value::String(val_str.to_string())
}

/// Parse CSV into a Value (Array of Maps or Array of Arrays)
pub fn parse_csv(
    input: &str,
    has_headers: bool,
    delimiter: char,
    typed: bool,
) -> Result<Value, String> {
    let raw_rows = parse_csv_raw(input, delimiter)?;
    if raw_rows.is_empty() {
        return Ok(Value::Array(Arc::new(RwLock::new(Vec::new()))));
    }

    if has_headers {
        let headers = &raw_rows[0];
        let mut result_rows = Vec::with_capacity(raw_rows.len().saturating_sub(1));

        for row in raw_rows.iter().skip(1) {
            let mut map = IndexMap::new();
            for (i, header) in headers.iter().enumerate() {
                let cell_str = row.get(i).map(|s| s.as_str()).unwrap_or("");
                map.insert(header.clone(), coerce_value(cell_str, typed));
            }
            result_rows.push(Value::Map(Arc::new(RwLock::new(map))));
        }

        Ok(Value::Array(Arc::new(RwLock::new(result_rows))))
    } else {
        let mut result_rows = Vec::with_capacity(raw_rows.len());
        for row in raw_rows {
            let cells: Vec<Value> = row
                .into_iter()
                .map(|s| coerce_value(&s, typed))
                .collect();
            result_rows.push(Value::Array(Arc::new(RwLock::new(cells))));
        }
        Ok(Value::Array(Arc::new(RwLock::new(result_rows))))
    }
}

/// Escape a single field according to RFC 4180
pub fn escape_csv_field(field: &str, delimiter: char) -> String {
    let needs_quotes = field.contains(delimiter)
        || field.contains('"')
        || field.contains('\n')
        || field.contains('\r');

    if needs_quotes {
        let escaped = field.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        field.to_string()
    }
}

/// Stringify a Value (Array of Maps or Array of Arrays) into RFC 4180 CSV
pub fn stringify_csv(
    rows_val: &Value,
    delimiter: char,
    custom_headers: Option<Vec<String>>,
) -> Result<String, String> {
    let arr_lock = match rows_val {
        Value::Array(arr) => arr.read().unwrap(),
        _ => return Err("csv.stringify expects an Array of Maps or Array of Arrays".to_string()),
    };

    if arr_lock.is_empty() {
        return Ok(String::new());
    }

    let first = &arr_lock[0];
    let mut out = String::new();

    if let Value::Map(first_map) = first {
        // Collect headers
        let headers: Vec<String> = if let Some(h) = custom_headers {
            h
        } else {
            first_map.read().unwrap().keys().cloned().collect()
        };

        // Write header row
        let header_line: Vec<String> = headers
            .iter()
            .map(|h| escape_csv_field(h, delimiter))
            .collect();
        out.push_str(&header_line.join(&delimiter.to_string()));
        out.push('\n');

        // Write records
        for item in arr_lock.iter() {
            if let Value::Map(m) = item {
                let map_guard = m.read().unwrap();
                let mut row_fields = Vec::with_capacity(headers.len());
                for h in &headers {
                    let cell_str = map_guard
                        .get(h)
                        .map(|v| v.to_display())
                        .unwrap_or_default();
                    row_fields.push(escape_csv_field(&cell_str, delimiter));
                }
                out.push_str(&row_fields.join(&delimiter.to_string()));
                out.push('\n');
            }
        }
    } else if let Value::Array(_) = first {
        for item in arr_lock.iter() {
            if let Value::Array(row_arr) = item {
                let guard = row_arr.read().unwrap();
                let row_fields: Vec<String> = guard
                    .iter()
                    .map(|cell| escape_csv_field(&cell.to_display(), delimiter))
                    .collect();
                out.push_str(&row_fields.join(&delimiter.to_string()));
                out.push('\n');
            }
        }
    } else {
        return Err("csv.stringify array elements must be Maps or Arrays".to_string());
    }

    Ok(out)
}
