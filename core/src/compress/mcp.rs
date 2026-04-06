use crate::CompressionLevel;
use serde_json::Value;

/// Compress MCP tool schema definitions.
///
/// Tiered approach:
/// - Active tools (recently used): full schema
/// - Available tools (not recently used): name + description only
/// - Grouped by namespace: "mcp__gmail: 7 tools available"
///
/// Since we can't track per-session tool usage in this stateless function,
/// we apply structural compression: collapse verbose input_schemas,
/// group by namespace, and remove redundant fields.
pub fn compress_mcp_schema(content: &str, level: CompressionLevel) -> String {
    let Ok(val) = serde_json::from_str::<Value>(content) else {
        return content.to_string();
    };

    match &val {
        Value::Array(tools) => compress_tool_list(tools, level),
        Value::Object(_) => compress_single_tool(&val, level),
        _ => content.to_string(),
    }
}

fn compress_tool_list(tools: &[Value], level: CompressionLevel) -> String {
    let max_full_schemas = match level {
        CompressionLevel::Light => 20,
        CompressionLevel::Medium => 10,
        CompressionLevel::Aggressive => 5,
    };

    if tools.len() <= max_full_schemas {
        // Few enough tools — compress each individually
        let compressed: Vec<Value> = tools
            .iter()
            .map(|t| compress_tool_schema(t, level))
            .collect();
        return serde_json::to_string_pretty(&compressed).unwrap_or_default();
    }

    // Group by namespace (prefix before double underscore)
    let mut namespaces: std::collections::BTreeMap<String, Vec<&Value>> =
        std::collections::BTreeMap::new();

    for tool in tools {
        let name = tool
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("unknown");
        let namespace = if let Some(idx) = name.find("__") {
            &name[..idx]
        } else {
            "core"
        };
        namespaces
            .entry(namespace.to_string())
            .or_default()
            .push(tool);
    }

    let mut result = Vec::new();
    let mut full_count = 0;

    for (ns, ns_tools) in &namespaces {
        if full_count >= max_full_schemas {
            // Just list the namespace with count
            let names: Vec<&str> = ns_tools
                .iter()
                .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
                .collect();
            result.push(format!(
                "[{ns}] {} tools: {}",
                ns_tools.len(),
                names.join(", ")
            ));
        } else {
            // Include compressed schemas
            for tool in ns_tools {
                if full_count < max_full_schemas {
                    let compressed = compress_tool_schema(tool, level);
                    result.push(serde_json::to_string(&compressed).unwrap_or_default());
                    full_count += 1;
                } else {
                    let name = tool
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("?");
                    let desc = tool
                        .get("description")
                        .and_then(|d| d.as_str())
                        .map(|d| truncate(d, 60))
                        .unwrap_or_default();
                    result.push(format!("  {name}: {desc}"));
                }
            }
        }
    }

    result.join("\n")
}

fn compress_single_tool(tool: &Value, level: CompressionLevel) -> String {
    let compressed = compress_tool_schema(tool, level);
    serde_json::to_string_pretty(&compressed).unwrap_or_default()
}

fn compress_tool_schema(tool: &Value, level: CompressionLevel) -> Value {
    let mut result = serde_json::Map::new();

    // Always keep name
    if let Some(name) = tool.get("name") {
        result.insert("name".to_string(), name.clone());
    }

    // Keep description but truncate for aggressive
    if let Some(desc) = tool.get("description").and_then(|d| d.as_str()) {
        let max_desc = match level {
            CompressionLevel::Light => 500,
            CompressionLevel::Medium => 200,
            CompressionLevel::Aggressive => 80,
        };
        result.insert(
            "description".to_string(),
            Value::String(truncate(desc, max_desc)),
        );
    }

    // Compress input_schema
    if let Some(schema) = tool.get("input_schema") {
        result.insert(
            "input_schema".to_string(),
            compress_schema(schema, level),
        );
    }

    // Drop output_schema entirely for aggressive
    if level != CompressionLevel::Aggressive {
        if let Some(output) = tool.get("output_schema") {
            result.insert("output_schema".to_string(), output.clone());
        }
    }

    Value::Object(result)
}

fn compress_schema(schema: &Value, level: CompressionLevel) -> Value {
    match level {
        CompressionLevel::Light => schema.clone(),
        CompressionLevel::Medium => {
            // Keep property names and types, drop descriptions
            strip_descriptions(schema)
        }
        CompressionLevel::Aggressive => {
            // Just list required params
            if let Some(props) = schema.get("properties") {
                if let Some(obj) = props.as_object() {
                    let params: Vec<String> = obj
                        .iter()
                        .map(|(k, v)| {
                            let t = v
                                .get("type")
                                .and_then(|t| t.as_str())
                                .unwrap_or("any");
                            format!("{k}: {t}")
                        })
                        .collect();
                    return Value::String(format!("({})", params.join(", ")));
                }
            }
            schema.clone()
        }
    }
}

fn strip_descriptions(val: &Value) -> Value {
    match val {
        Value::Object(obj) => {
            let mut result = serde_json::Map::new();
            for (k, v) in obj {
                if k == "description" {
                    continue;
                }
                result.insert(k.clone(), strip_descriptions(v));
            }
            Value::Object(result)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(strip_descriptions).collect()),
        other => other.clone(),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        // Find a char boundary at or before `max` to avoid panicking on multi-byte UTF-8.
        // Slicing directly at `max` will panic if it falls inside a multi-byte character.
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}
