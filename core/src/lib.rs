pub mod bench;
pub mod compress;
pub mod context;
pub mod learning;
pub mod mcp_server;
pub mod quality;
pub mod setup;
pub mod utils;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Programming language for AST-aware compression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Rust,
    TypeScript,
    JavaScript,
    Python,
    Go,
    Java,
    C,
    Cpp,
    Ruby,
    Php,
    Swift,
    Kotlin,
    CSharp,
    Bash,
}

impl Language {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "rs" => Some(Self::Rust),
            "ts" | "tsx" => Some(Self::TypeScript),
            "js" | "jsx" | "mjs" | "cjs" => Some(Self::JavaScript),
            "py" | "pyi" => Some(Self::Python),
            "go" => Some(Self::Go),
            "java" => Some(Self::Java),
            "c" | "h" => Some(Self::C),
            "cpp" | "cc" | "cxx" | "hpp" | "hxx" => Some(Self::Cpp),
            "rb" => Some(Self::Ruby),
            "php" => Some(Self::Php),
            "swift" => Some(Self::Swift),
            "kt" | "kts" => Some(Self::Kotlin),
            "cs" => Some(Self::CSharp),
            "sh" | "bash" | "zsh" => Some(Self::Bash),
            _ => None,
        }
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Rust => "rust",
            Self::TypeScript => "typescript",
            Self::JavaScript => "javascript",
            Self::Python => "python",
            Self::Go => "go",
            Self::Java => "java",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Ruby => "ruby",
            Self::Php => "php",
            Self::Swift => "swift",
            Self::Kotlin => "kotlin",
            Self::CSharp => "csharp",
            Self::Bash => "bash",
        };
        write!(f, "{s}")
    }
}

/// Detected content type for routing through compression pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentType {
    Code { language: Language },
    CommandOutput,
    Conversation,
    Json,
    McpSchema,
    Unknown,
}

impl std::fmt::Display for ContentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Code { language } => write!(f, "code:{language}"),
            Self::CommandOutput => write!(f, "command_output"),
            Self::Conversation => write!(f, "conversation"),
            Self::Json => write!(f, "json"),
            Self::McpSchema => write!(f, "mcp_schema"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Compression aggressiveness level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompressionLevel {
    Light,
    Medium,
    Aggressive,
}

impl Default for CompressionLevel {
    fn default() -> Self {
        Self::Medium
    }
}

/// Result of compressing content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressResult {
    pub compressed: String,
    pub original_tokens: usize,
    pub compressed_tokens: usize,
    pub ratio: f64,
    pub content_type: ContentType,
    pub original_hash: String,
}

/// Per-category token budget configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetConfig {
    pub total: usize,
    pub conversation: f32,
    pub tool_output: f32,
    pub code_context: f32,
    pub mcp_schema: f32,
    pub auto_adjust: bool,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            total: 100_000,
            conversation: 0.40,
            tool_output: 0.30,
            code_context: 0.20,
            mcp_schema: 0.10,
            auto_adjust: true,
        }
    }
}

/// Hook input — JSON received from PostToolUse hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookInput {
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: serde_json::Value,
    #[serde(default)]
    pub tool_output: String,
}

/// Session statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStats {
    pub session_id: String,
    pub total_original_tokens: usize,
    pub total_compressed_tokens: usize,
    pub tokens_saved: usize,
    pub overall_ratio: f64,
    pub compressions_count: usize,
    pub by_type: Vec<TypeStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeStats {
    pub content_type: String,
    pub original_tokens: usize,
    pub compressed_tokens: usize,
    pub ratio: f64,
    pub count: usize,
}

/// Top-level engine.
pub struct Engine {
    db_path: PathBuf,
    level: CompressionLevel,
    budget: BudgetConfig,
}

impl Engine {
    pub fn new(db_path: PathBuf) -> Self {
        Self {
            db_path,
            level: CompressionLevel::default(),
            budget: BudgetConfig::default(),
        }
    }

    pub fn with_level(mut self, level: CompressionLevel) -> Self {
        self.level = level;
        self
    }

    pub fn with_budget(mut self, budget: BudgetConfig) -> Self {
        self.budget = budget;
        self
    }

    /// Compress content, auto-detecting type.
    pub fn compress(&self, content: &str, type_hint: Option<ContentType>) -> anyhow::Result<CompressResult> {
        let content_type = type_hint.unwrap_or_else(|| compress::detect_content_type(content));
        let original_tokens = utils::tokens::count_tokens(content);
        let original_hash = blake3::hash(content.as_bytes()).to_hex().to_string();

        let compressed = match &content_type {
            ContentType::Code { language } => {
                compress::code::compress_code(content, *language, self.level)
            }
            ContentType::CommandOutput => {
                compress::output::compress_output(content, self.level)
            }
            ContentType::Json => {
                compress::json::compress_json(content, self.level)
            }
            ContentType::Conversation => {
                compress::conversation::compress_conversation(content, self.level)
            }
            ContentType::McpSchema => {
                compress::mcp::compress_mcp_schema(content, self.level)
            }
            ContentType::Unknown => {
                // Fallback: basic line dedup + truncation
                compress::output::compress_output(content, self.level)
            }
        };

        let compressed_tokens = utils::tokens::count_tokens(&compressed);
        let ratio = if original_tokens > 0 {
            1.0 - (compressed_tokens as f64 / original_tokens as f64)
        } else {
            0.0
        };

        // Store in database
        if let Ok(store) = context::store::Store::open(&self.db_path) {
            let _ = store.record_compression(
                "current",
                &content_type.to_string(),
                &original_hash,
                content,
                &compressed,
                original_tokens,
                compressed_tokens,
            );
        }

        Ok(CompressResult {
            compressed,
            original_tokens,
            compressed_tokens,
            ratio,
            content_type,
            original_hash,
        })
    }

    /// Process a PostToolUse hook input.
    pub fn process_hook(&self, input: &HookInput, session_id: &str) -> anyhow::Result<CompressResult> {
        let type_hint = content_type_from_tool(&input.tool_name, &input.tool_output);
        let result = self.compress(&input.tool_output, Some(type_hint))?;

        if let Ok(store) = context::store::Store::open(&self.db_path) {
            let _ = store.record_compression(
                session_id,
                &result.content_type.to_string(),
                &result.original_hash,
                &input.tool_output,
                &result.compressed,
                result.original_tokens,
                result.compressed_tokens,
            );
        }

        Ok(result)
    }

    /// Get session statistics.
    pub fn stats(&self, session_id: &str) -> anyhow::Result<SessionStats> {
        let store = context::store::Store::open(&self.db_path)?;
        store.session_stats(session_id)
    }

    /// Expand a previously compressed item by hash.
    pub fn expand(&self, content_hash: &str) -> anyhow::Result<String> {
        let store = context::store::Store::open(&self.db_path)?;
        store.get_original(content_hash)
    }
}

/// Infer content type from tool name and output.
fn content_type_from_tool(tool_name: &str, output: &str) -> ContentType {
    match tool_name {
        "Read" => {
            // Try to detect language from content
            compress::detect_content_type(output)
        }
        "Bash" | "Grep" | "Glob" => ContentType::CommandOutput,
        "Edit" | "Write" => ContentType::Code {
            language: Language::Rust, // will be refined by detection
        },
        _ => compress::detect_content_type(output),
    }
}
