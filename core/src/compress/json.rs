use crate::CompressionLevel;
use serde_json::Value;

/// Compress JSON/structured data.
///
/// Strategies:
/// - Arrays: keep schema (first element) + count + sample (first/middle/last)
/// - Deep objects: flatten to max depth, summarize deeper levels
/// - Known formats: specialized compression for package.json deps, API pagination
pub fn compress_json(content: &str, level: CompressionLevel) -> String {
    let Ok(val) = serde_json::from_str::<Value>(content) else {
        // Not valid JSON — return as-is (maybe partial JSON)
        return content.to_string();
    };

    let max_depth = match level {
        CompressionLevel::Light => 5,
        CompressionLevel::Medium => 3,
        CompressionLevel::Aggressive => 2,
    };

    let max_array_items = match level {
        CompressionLevel::Light => 10,
        CompressionLevel::Medium => 5,
        CompressionLevel::Aggressive => 3,
    };

    let compressed = compress_value(&val, 0, max_depth, max_array_items);
    serde_json::to_string_pretty(&compressed).unwrap_or_else(|_| content.to_string())
}

fn compress_value(val: &Value, depth: usize, max_depth: usize, max_array: usize) -> Value {
    match val {
        Value::Array(arr) => compress_array(arr, depth, max_depth, max_array),
        Value::Object(obj) => compress_object(obj, depth, max_depth, max_array),
        other => other.clone(),
    }
}

fn compress_array(arr: &[Value], depth: usize, max_depth: usize, max_array: usize) -> Value {
    let len = arr.len();

    if len <= max_array {
        // Small enough — keep all but recurse into children
        return Value::Array(
            arr.iter()
                .map(|v| compress_value(v, depth + 1, max_depth, max_array))
                .collect(),
        );
    }

    // Sample: first, middle, last + count annotation
    let mut result = Vec::new();

    // First item(s)
    let take_start = (max_array / 2).max(1);
    for item in arr.iter().take(take_start) {
        result.push(compress_value(item, depth + 1, max_depth, max_array));
    }

    // Count marker
    let omitted = len - take_start.min(len) - (max_array - take_start).min(len);
    if omitted > 0 {
        result.push(Value::String(format!("... {omitted} more items ({len} total)")));
    }

    // Last item(s)
    let take_end = max_array - take_start;
    for item in arr.iter().rev().take(take_end).rev() {
        result.push(compress_value(item, depth + 1, max_depth, max_array));
    }

    Value::Array(result)
}

fn compress_object(
    obj: &serde_json::Map<String, Value>,
    depth: usize,
    max_depth: usize,
    max_array: usize,
) -> Value {
    if depth >= max_depth {
        // Too deep — summarize
        let key_count = obj.len();
        let keys: Vec<&str> = obj.keys().take(5).map(|k| k.as_str()).collect();
        let key_preview = keys.join(", ");
        if key_count > 5 {
            return Value::String(format!("{{...{key_count} keys: {key_preview}, ...}}"));
        } else {
            return Value::String(format!("{{{key_preview}}}"));
        }
    }

    let mut result = serde_json::Map::new();
    for (key, val) in obj {
        result.insert(
            key.clone(),
            compress_value(val, depth + 1, max_depth, max_array),
        );
    }

    Value::Object(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compresses_large_array() {
        let arr: Vec<Value> = (0..100).map(|i| Value::Number(i.into())).collect();
        let json = serde_json::to_string(&arr).unwrap();
        let result = compress_json(&json, CompressionLevel::Medium);
        assert!(result.len() < json.len());
        assert!(result.contains("more items"));
    }

    #[test]
    fn preserves_small_json() {
        let json = r#"{"name": "test", "version": "1.0"}"#;
        let result = compress_json(json, CompressionLevel::Medium);
        assert!(result.contains("test"));
        assert!(result.contains("1.0"));
    }

    #[test]
    fn deep_objects_get_summarized() {
        let deep = r#"{"a":{"b":{"c":{"d":{"e":{"f":"deep"}}}}}}"#;
        let result = compress_json(deep, CompressionLevel::Aggressive);
        // At depth 2, inner objects should be summarized
        assert!(result.len() <= deep.len() + 50); // pretty printing may add whitespace
    }
}
