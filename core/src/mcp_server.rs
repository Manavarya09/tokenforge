//! TokenForge as an MCP server — exposes compress, expand, stats as MCP tools
//! over JSON-RPC 2.0 / stdio.
//!
//! Start with: `tokenforge serve`
//! Configure in claude_desktop_config.json:
//!   {
//!     "mcpServers": {
//!       "tokenforge": {
//!         "command": "tokenforge",
//!         "args": ["serve"]
//!       }
//!     }
//!   }

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use crate::{ContentType, Engine};

pub struct McpServer {
    db_path: PathBuf,
}

impl McpServer {
    pub fn new(db_path: PathBuf) -> Self {
        Self { db_path }
    }

    /// Run the MCP server loop — reads JSON-RPC from stdin, writes to stdout.
    pub fn run(&self) -> Result<()> {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let mut out = stdout.lock();

        let reader = BufReader::new(stdin.lock());

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let msg: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    let err_resp = json!({
                        "jsonrpc": "2.0",
                        "id": null,
                        "error": { "code": -32700, "message": format!("Parse error: {e}") }
                    });
                    writeln!(out, "{}", serde_json::to_string(&err_resp)?)?;
                    out.flush()?;
                    continue;
                }
            };

            let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
            let id = msg.get("id").cloned().unwrap_or(Value::Null);
            let params = msg.get("params").cloned().unwrap_or(Value::Null);

            // Notifications (no id) — acknowledge but don't respond
            if msg.get("id").is_none() {
                continue;
            }

            let response = match method {
                "initialize" => self.handle_initialize(id, &params),
                "tools/list" => self.handle_tools_list(id),
                "tools/call" => self.handle_tools_call(id, &params),
                "ping" => json!({ "jsonrpc": "2.0", "id": id, "result": {} }),
                _ => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32601, "message": format!("Method not found: {method}") }
                }),
            };

            writeln!(out, "{}", serde_json::to_string(&response)?)?;
            out.flush()?;
        }

        Ok(())
    }

    fn handle_initialize(&self, id: Value, _params: &Value) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "tokenforge",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        })
    }

    fn handle_tools_list(&self, id: Value) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "tools": [
                    {
                        "name": "tokenforge_compress",
                        "description": "Compress text, code, JSON, or command output to reduce token count. Returns compressed content with hash for lossless retrieval.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "content": {
                                    "type": "string",
                                    "description": "Content to compress"
                                },
                                "type": {
                                    "type": "string",
                                    "enum": ["code", "output", "conversation", "json", "mcp"],
                                    "description": "Content type (auto-detected if omitted)"
                                },
                                "level": {
                                    "type": "string",
                                    "enum": ["light", "medium", "aggressive"],
                                    "description": "Compression aggressiveness (default: medium)"
                                }
                            },
                            "required": ["content"]
                        }
                    },
                    {
                        "name": "tokenforge_expand",
                        "description": "Retrieve the original full content by its blake3 hash. Use when compressed output is missing information you need.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "hash": {
                                    "type": "string",
                                    "description": "Blake3 hash returned from tokenforge_compress"
                                }
                            },
                            "required": ["hash"]
                        }
                    },
                    {
                        "name": "tokenforge_stats",
                        "description": "Show token compression statistics for the current session.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "session_id": {
                                    "type": "string",
                                    "description": "Session ID (default: 'current')"
                                }
                            }
                        }
                    },
                    {
                        "name": "tokenforge_bench",
                        "description": "Run built-in benchmarks across all compression engines and return performance metrics.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "level": {
                                    "type": "string",
                                    "enum": ["light", "medium", "aggressive"],
                                    "description": "Compression level to benchmark"
                                }
                            }
                        }
                    }
                ]
            }
        })
    }

    fn handle_tools_call(&self, id: Value, params: &Value) -> Value {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("");
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or(Value::Object(Default::default()));

        let (content_text, is_error) = match name {
            "tokenforge_compress" => self.tool_compress(&args),
            "tokenforge_expand"   => self.tool_expand(&args),
            "tokenforge_stats"    => self.tool_stats(&args),
            "tokenforge_bench"    => self.tool_bench(&args),
            _ => (format!("Unknown tool: {name}"), true),
        };

        json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "content": [{ "type": "text", "text": content_text }],
                "isError": is_error
            }
        })
    }

    fn tool_compress(&self, args: &Value) -> (String, bool) {
        let content = match args.get("content").and_then(Value::as_str) {
            Some(c) => c,
            None => return ("Missing required field: content".to_string(), true),
        };

        let level_str = args.get("level").and_then(Value::as_str).unwrap_or("medium");
        let level = match level_str {
            "light"      => crate::CompressionLevel::Light,
            "aggressive" => crate::CompressionLevel::Aggressive,
            _            => crate::CompressionLevel::Medium,
        };

        let type_hint: Option<ContentType> = args.get("type").and_then(Value::as_str).map(|t| {
            match t {
                "code"    => ContentType::Code { language: crate::Language::Rust },
                "output"  => ContentType::CommandOutput,
                "convers" | "conversation" => ContentType::Conversation,
                "json"    => ContentType::Json,
                "mcp"     => ContentType::McpSchema,
                _         => ContentType::Unknown,
            }
        });

        let engine = Engine::new(self.db_path.clone()).with_level(level);
        match engine.compress(content, type_hint) {
            Ok(result) => {
                let out = serde_json::json!({
                    "compressed": result.compressed,
                    "original_tokens": result.original_tokens,
                    "compressed_tokens": result.compressed_tokens,
                    "ratio": format!("{:.1}%", result.ratio * 100.0),
                    "hash": result.original_hash,
                    "content_type": result.content_type.to_string()
                });
                (serde_json::to_string_pretty(&out).unwrap_or_default(), false)
            }
            Err(e) => (format!("Compression error: {e}"), true),
        }
    }

    fn tool_expand(&self, args: &Value) -> (String, bool) {
        let hash = match args.get("hash").and_then(Value::as_str) {
            Some(h) => h,
            None => return ("Missing required field: hash".to_string(), true),
        };

        let engine = Engine::new(self.db_path.clone());
        match engine.expand(hash) {
            Ok(original) => (original, false),
            Err(e) => (format!("Expand error: {e}"), true),
        }
    }

    fn tool_stats(&self, args: &Value) -> (String, bool) {
        let session = args
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("current");

        let engine = Engine::new(self.db_path.clone());
        match engine.stats(session) {
            Ok(stats) => {
                match serde_json::to_string_pretty(&stats) {
                    Ok(s) => (s, false),
                    Err(e) => (format!("Serialization error: {e}"), true),
                }
            }
            Err(e) => (format!("Stats error: {e}"), true),
        }
    }

    fn tool_bench(&self, args: &Value) -> (String, bool) {
        let level_str = args.get("level").and_then(Value::as_str).unwrap_or("medium");
        let level = match level_str {
            "light"      => crate::CompressionLevel::Light,
            "aggressive" => crate::CompressionLevel::Aggressive,
            _            => crate::CompressionLevel::Medium,
        };

        match crate::bench::run_bench(&self.db_path, level) {
            Ok(results) => {
                match serde_json::to_string_pretty(&results) {
                    Ok(s) => (s, false),
                    Err(e) => (format!("Serialization error: {e}"), true),
                }
            }
            Err(e) => (format!("Bench error: {e}"), true),
        }
    }
}

// ─── Serde types ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
#[allow(dead_code)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
#[allow(dead_code)]
struct JsonRpcError {
    code: i32,
    message: String,
}
