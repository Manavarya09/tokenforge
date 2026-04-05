//! Built-in benchmarking suite — proves TokenForge performance claims.
//!
//! Runs all compression engines against representative samples,
//! measures timing (ns/token) and compression ratios.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Instant;

use crate::{CompressionLevel, ContentType, Engine, Language};

#[derive(Debug, Serialize, Deserialize)]
pub struct BenchResult {
    pub engine: String,
    pub content_type: String,
    pub original_tokens: usize,
    pub compressed_tokens: usize,
    pub tokens_saved: usize,
    pub savings_pct: f64,
    pub quality_score: f64,
    pub median_ns: u64,
    pub ns_per_token: f64,
}

/// Run all compression benchmarks and return results.
pub fn run_bench(db_path: &Path, level: CompressionLevel) -> Result<Vec<BenchResult>> {
    let samples = make_samples();
    let mut results = Vec::new();

    for (name, content_type, content) in &samples {
        let result = bench_one(name, content_type.clone(), content, level, db_path)?;
        results.push(result);
    }

    Ok(results)
}

/// Format bench results as a human-readable table.
pub fn format_table(results: &[BenchResult]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "\n{:<28} {:<14} {:>7}  {:>7}  {:>7}  {:>8}  {:>9}\n",
        "Engine", "Type", "Tokens", "→ After", "Saved%", "Quality", "ns/token"
    ));
    out.push_str(&"─".repeat(90));
    out.push('\n');

    for r in results {
        out.push_str(&format!(
            "{:<28} {:<14} {:>7}  {:>7}  {:>6.1}%  {:>7.1}   {:>8.0}\n",
            r.engine,
            r.content_type,
            r.original_tokens,
            r.compressed_tokens,
            r.savings_pct,
            r.quality_score,
            r.ns_per_token
        ));
    }

    out.push('\n');
    out
}

// ─── Internal ──────────────────────────────────────────────────────────────

fn bench_one(
    name: &str,
    content_type: ContentType,
    content: &str,
    level: CompressionLevel,
    db_path: &Path,
) -> Result<BenchResult> {
    const RUNS: usize = 3;
    let mut timings = Vec::with_capacity(RUNS);
    let mut last_result = None;

    let engine = Engine::new(db_path.to_path_buf()).with_level(level);

    for _ in 0..RUNS {
        let t0 = Instant::now();
        let result = engine.compress(content, Some(content_type.clone()))?;
        timings.push(t0.elapsed().as_nanos() as u64);
        last_result = Some(result);
    }

    timings.sort_unstable();
    let median_ns = timings[RUNS / 2];
    let result = last_result.unwrap();

    let original_tokens = result.original_tokens.max(1);
    let ns_per_token = median_ns as f64 / original_tokens as f64;
    let tokens_saved = result.original_tokens.saturating_sub(result.compressed_tokens);
    // result.ratio = savings fraction (1 - comp/orig), so 0 = no savings, 1 = all saved
    let savings_pct = result.ratio * 100.0;

    // Quality: ratio is savings fraction; sweet spot 0.3–0.8
    let quality_score = quality_estimate(result.ratio);

    Ok(BenchResult {
        engine: name.to_string(),
        content_type: result.content_type.to_string(),
        original_tokens,
        compressed_tokens: result.compressed_tokens,
        tokens_saved,
        savings_pct,
        quality_score,
        median_ns,
        ns_per_token,
    })
}

fn quality_estimate(ratio: f64) -> f64 {
    // ratio = savings fraction: 0 = no savings, 1 = everything removed.
    // Sweet spot: 0.3–0.8 savings. Penalise too little (<5%) or too much (>95%).
    let base = 85.0_f64;
    let score = if ratio < 0.05 {
        base - 15.0 // barely saved anything
    } else if ratio > 0.95 {
        base - 10.0 // possibly over-compressed / lossy
    } else if ratio >= 0.3 && ratio <= 0.8 {
        base + 10.0 // excellent sweet spot
    } else {
        base + 3.0 // acceptable
    };
    score.clamp(0.0, 100.0)
}

/// Build a representative sample corpus for all 5 engines.
fn make_samples() -> Vec<(&'static str, ContentType, &'static str)> {
    vec![
        (
            "code:rust (AST)",
            ContentType::Code { language: Language::Rust },
            RUST_SAMPLE,
        ),
        (
            "code:python (AST)",
            ContentType::Code { language: Language::Python },
            PYTHON_SAMPLE,
        ),
        (
            "code:typescript",
            ContentType::Code { language: Language::TypeScript },
            TYPESCRIPT_SAMPLE,
        ),
        (
            "command_output",
            ContentType::CommandOutput,
            COMMAND_OUTPUT_SAMPLE,
        ),
        (
            "conversation",
            ContentType::Conversation,
            CONVERSATION_SAMPLE,
        ),
        (
            "json_payload",
            ContentType::Json,
            JSON_SAMPLE,
        ),
        (
            "mcp_schema",
            ContentType::McpSchema,
            MCP_SCHEMA_SAMPLE,
        ),
    ]
}

// ─── Sample corpora ───────────────────────────────────────────────────────────

static RUST_SAMPLE: &str = r#"
use std::collections::HashMap;
use std::path::PathBuf;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// A full-featured configuration manager with layered overrides.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigManager {
    base: HashMap<String, serde_json::Value>,
    overrides: Vec<HashMap<String, serde_json::Value>>,
    path: PathBuf,
}

impl ConfigManager {
    pub fn new(path: PathBuf) -> Self {
        Self { base: HashMap::new(), overrides: Vec::new(), path }
    }

    pub fn load(&mut self) -> Result<()> {
        let data = std::fs::read_to_string(&self.path)
            .with_context(|| format!("failed to read config from {:?}", self.path))?;
        self.base = serde_json::from_str(&data)
            .context("failed to parse config JSON")?;
        Ok(())
    }

    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        for layer in self.overrides.iter().rev() {
            if let Some(v) = layer.get(key) {
                return Some(v);
            }
        }
        self.base.get(key)
    }

    pub fn set_override(&mut self, key: String, value: serde_json::Value) {
        if self.overrides.is_empty() {
            self.overrides.push(HashMap::new());
        }
        self.overrides.last_mut().unwrap().insert(key, value);
    }

    pub fn merge_layer(&mut self, layer: HashMap<String, serde_json::Value>) {
        self.overrides.push(layer);
    }

    pub fn save(&self) -> Result<()> {
        let mut merged = self.base.clone();
        for layer in &self.overrides {
            for (k, v) in layer {
                merged.insert(k.clone(), v.clone());
            }
        }
        let json = serde_json::to_string_pretty(&merged)
            .context("serialization failed")?;
        std::fs::write(&self.path, json)
            .with_context(|| format!("failed to write config to {:?}", self.path))?;
        Ok(())
    }

    pub fn keys(&self) -> Vec<&str> {
        let mut keys: std::collections::HashSet<&str> = self.base.keys().map(|s| s.as_str()).collect();
        for layer in &self.overrides {
            for k in layer.keys() {
                keys.insert(k.as_str());
            }
        }
        let mut v: Vec<&str> = keys.into_iter().collect();
        v.sort_unstable();
        v
    }

    pub fn reset_overrides(&mut self) {
        self.overrides.clear();
    }

    pub fn has_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    pub fn remove(&mut self, key: &str) -> bool {
        let was_in_base = self.base.remove(key).is_some();
        let was_in_override = self.overrides.iter_mut().any(|l| l.remove(key).is_some());
        was_in_base || was_in_override
    }
}

impl Default for ConfigManager {
    fn default() -> Self {
        Self::new(PathBuf::from("config.json"))
    }
}

pub fn load_from_env() -> Result<ConfigManager> {
    let path = std::env::var("CONFIG_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("config.json"));
    let mut cfg = ConfigManager::new(path);
    if cfg.path.exists() {
        cfg.load()?;
    }
    Ok(cfg)
}
"#;

static PYTHON_SAMPLE: &str = r#"
import os
import json
import hashlib
from pathlib import Path
from typing import Optional, Dict, List, Any
from dataclasses import dataclass, field

@dataclass
class CacheEntry:
    key: str
    value: Any
    hits: int = 0
    ttl_seconds: Optional[int] = None

class SmartCache:
    """LRU cache with TTL, persistence, and stats tracking."""

    def __init__(self, capacity: int = 1000, persist_path: Optional[Path] = None):
        self.capacity = capacity
        self.persist_path = persist_path
        self._store: Dict[str, CacheEntry] = {}
        self._access_order: List[str] = []
        if persist_path and persist_path.exists():
            self._load_from_disk()

    def get(self, key: str) -> Optional[Any]:
        if key not in self._store:
            return None
        entry = self._store[key]
        entry.hits += 1
        self._access_order.remove(key)
        self._access_order.append(key)
        return entry.value

    def set(self, key: str, value: Any, ttl: Optional[int] = None) -> None:
        if len(self._store) >= self.capacity and key not in self._store:
            evict = self._access_order.pop(0)
            del self._store[evict]
        self._store[key] = CacheEntry(key=key, value=value, ttl_seconds=ttl)
        if key in self._access_order:
            self._access_order.remove(key)
        self._access_order.append(key)

    def invalidate(self, key: str) -> bool:
        if key in self._store:
            del self._store[key]
            self._access_order.remove(key)
            return True
        return False

    def stats(self) -> Dict[str, Any]:
        total_hits = sum(e.hits for e in self._store.values())
        return {
            "size": len(self._store),
            "capacity": self.capacity,
            "utilization": len(self._store) / self.capacity,
            "total_hits": total_hits,
            "keys": list(self._store.keys()),
        }

    def _load_from_disk(self) -> None:
        try:
            with open(self.persist_path) as f:
                data = json.load(f)
            for key, item in data.items():
                self._store[key] = CacheEntry(
                    key=key, value=item["value"], hits=item.get("hits", 0)
                )
                self._access_order.append(key)
        except (json.JSONDecodeError, KeyError, IOError):
            pass

    def save_to_disk(self) -> None:
        if not self.persist_path:
            return
        data = {
            k: {"value": e.value, "hits": e.hits}
            for k, e in self._store.items()
        }
        self.persist_path.parent.mkdir(parents=True, exist_ok=True)
        with open(self.persist_path, "w") as f:
            json.dump(data, f, indent=2)
"#;

static TYPESCRIPT_SAMPLE: &str = r#"
import { EventEmitter } from 'events';

interface TaskOptions {
    retries?: number;
    timeout?: number;
    priority?: 'low' | 'normal' | 'high';
}

interface TaskResult<T> {
    data: T;
    duration: number;
    attempts: number;
}

type TaskFn<T> = () => Promise<T>;

export class TaskQueue extends EventEmitter {
    private queue: Array<{ fn: TaskFn<unknown>; opts: TaskOptions; resolve: Function; reject: Function }> = [];
    private running = 0;
    private readonly concurrency: number;

    constructor(concurrency = 4) {
        super();
        this.concurrency = concurrency;
    }

    async add<T>(fn: TaskFn<T>, opts: TaskOptions = {}): Promise<TaskResult<T>> {
        return new Promise((resolve, reject) => {
            this.queue.push({ fn: fn as TaskFn<unknown>, opts, resolve, reject });
            this.queue.sort((a, b) => {
                const priority = { high: 2, normal: 1, low: 0 };
                return (priority[b.opts.priority ?? 'normal'] ?? 1) -
                       (priority[a.opts.priority ?? 'normal'] ?? 1);
            });
            this.drain();
        });
    }

    private async drain(): Promise<void> {
        while (this.running < this.concurrency && this.queue.length > 0) {
            const task = this.queue.shift()!;
            this.running++;
            this.execute(task);
        }
    }

    private async execute(task: { fn: TaskFn<unknown>; opts: TaskOptions; resolve: Function; reject: Function }): Promise<void> {
        const { fn, opts, resolve, reject } = task;
        const maxRetries = opts.retries ?? 0;
        const timeout = opts.timeout ?? 30_000;
        let attempts = 0;
        const start = Date.now();
        while (attempts <= maxRetries) {
            try {
                const result = await Promise.race([
                    fn(),
                    new Promise((_, r) => setTimeout(() => r(new Error('timeout')), timeout))
                ]);
                this.running--;
                this.emit('task:complete', { attempts: attempts + 1 });
                resolve({ data: result, duration: Date.now() - start, attempts: attempts + 1 });
                this.drain();
                return;
            } catch (err) {
                attempts++;
                if (attempts > maxRetries) {
                    this.running--;
                    this.emit('task:error', err);
                    reject(err);
                    this.drain();
                    return;
                }
                await new Promise(r => setTimeout(r, 100 * attempts));
            }
        }
    }

    get pending(): number { return this.queue.length; }
    get active(): number { return this.running; }
}
"#;

static COMMAND_OUTPUT_SAMPLE: &str = r#"
running 47 tests
test compress::code::tests::small_files_not_compressed ... ok
test compress::code::tests::large_function_gets_folded ... ok
test compress::output::tests::ansi_stripped ... ok
test compress::output::tests::test_output_summarized ... ok
test compress::output::tests::compiler_output_summarized ... ok
test compress::output::tests::dedup_similar_lines ... ok
test compress::json::tests::small_json_passthrough ... ok
test compress::json::tests::large_array_sampled ... ok
test compress::json::tests::deep_json_depth_limited ... ok
test compress::conversation::tests::empty_passthrough ... ok
test context::store::tests::roundtrip ... ok
test context::store::tests::session_stats_empty ... ok
test compress::diff::tests::jaccard_identical ... ok
test compress::diff::tests::jaccard_disjoint ... ok
test compress::diff::tests::roundtrip_encode_parse ... ok
test bench::tests::bench_smoke ... ok
warning: 3 warnings emitted

test result: ok. 47 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.14s

   Compiling tokenforge v0.1.1 (/Users/dev/tokenforge/core)
warning: unused variable `sig`
  --> src/compress/code.rs:64:18
   |
64 |             let (sig, body_start, body_end) = find_block_extent(&lines, i, language);
   |                  ^^^ help: if this is intentional, prefix it with an underscore: `_sig`
warning: value assigned to `run_count` is never read
warning: unused import: `Language`
 --> src/compress/mod.rs:7:26
warning: `tokenforge` (lib) generated 3 warnings
    Finished `release` profile [optimized] target(s) in 34.84s
"#;

static CONVERSATION_SAMPLE: &str = "\
User: I need to implement a rate limiter for our API.\n\
Assistant: Use a token bucket. Track per-IP state in a HashMap, refill at R tokens/sec, reject when empty.\n\
User: Should we persist state across restarts?\n\
Assistant: For a REST API, stateless is usually fine. Use Redis if you need cross-instance sharing.\n\
User: What about burst handling?\n\
Assistant: Token bucket naturally handles bursts up to the bucket size. Set bucket_size = 10 * rate for generous bursts.\n\
User: Can you show me the Rust code?\n\
Assistant: Here is a production-ready implementation using DashMap for concurrent access without a global lock.\n\
User: How do I integrate this with Axum?\n\
Assistant: Add it as tower middleware. Implement the Service trait, check the bucket in poll_ready, return 429 if exhausted.\n\
User: What about the X-RateLimit headers?\n\
Assistant: Inject X-RateLimit-Limit, X-RateLimit-Remaining, and Retry-After headers in the response layer.\n\
User: Tests?\n\
Assistant: Test edge cases: exactly-full bucket, burst at limit, refill timing, concurrent requests, and the 429 response format.\
";

static JSON_SAMPLE: &str = r#"{
  "openapi": "3.0.0",
  "info": { "title": "TokenForge API", "version": "1.0.0" },
  "paths": {
    "/compress": {
      "post": {
        "summary": "Compress content",
        "requestBody": {
          "content": {
            "application/json": {
              "schema": {
                "type": "object",
                "properties": {
                  "content": { "type": "string" },
                  "type": { "type": "string", "enum": ["code","output","json","conversation","mcp"] },
                  "level": { "type": "string", "enum": ["light","medium","aggressive"] }
                },
                "required": ["content"]
              }
            }
          }
        },
        "responses": {
          "200": {
            "description": "Compressed result",
            "content": {
              "application/json": {
                "schema": {
                  "type": "object",
                  "properties": {
                    "compressed": { "type": "string" },
                    "original_tokens": { "type": "integer" },
                    "compressed_tokens": { "type": "integer" },
                    "ratio": { "type": "number" },
                    "hash": { "type": "string" }
                  }
                }
              }
            }
          }
        }
      }
    }
  }
}"#;

static MCP_SCHEMA_SAMPLE: &str = r#"[
  {"name":"read_file","description":"Read file contents","input_schema":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}},
  {"name":"write_file","description":"Write file contents","input_schema":{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]}},
  {"name":"bash","description":"Run a bash command","input_schema":{"type":"object","properties":{"command":{"type":"string"}},"required":["command"]}},
  {"name":"search","description":"Search codebase","input_schema":{"type":"object","properties":{"pattern":{"type":"string"},"path":{"type":"string"}},"required":["pattern"]}},
  {"name":"glob","description":"Find files by pattern","input_schema":{"type":"object","properties":{"pattern":{"type":"string"}},"required":["pattern"]}},
  {"name":"edit_file","description":"Edit file contents","input_schema":{"type":"object","properties":{"path":{"type":"string"},"old":{"type":"string"},"new":{"type":"string"}},"required":["path","old","new"]}},
  {"name":"create_file","description":"Create a new file","input_schema":{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]}},
  {"name":"list_dir","description":"List directory contents","input_schema":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}},
  {"name":"move_file","description":"Move or rename a file","input_schema":{"type":"object","properties":{"src":{"type":"string"},"dst":{"type":"string"}},"required":["src","dst"]}},
  {"name":"delete_file","description":"Delete a file","input_schema":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}
]"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn bench_smoke() {
        let db = PathBuf::from("/tmp/tokenforge_bench_test.db");
        let results = run_bench(&db, CompressionLevel::Medium).unwrap();
        assert!(!results.is_empty());
        for r in &results {
            assert!(r.original_tokens > 0);
            assert!(r.ns_per_token >= 0.0);
        }
        let _ = std::fs::remove_file(&db);
    }
}
