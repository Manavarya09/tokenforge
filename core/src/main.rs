use anyhow::Result;
use clap::{Parser, Subcommand};
use std::io::Read;
use std::path::PathBuf;

use tokenforge::{
    bench, mcp_server, setup, CompressionLevel, ContentType, Engine, HookInput, Language,
};

#[derive(Parser)]
#[command(
    name = "tokenforge",
    about = "TokenForge — Full-stack LLM token optimization engine",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Output as JSON
    #[arg(long, global = true)]
    json: bool,

    /// Verbose logging
    #[arg(long, short, global = true)]
    verbose: bool,

    /// SQLite database path
    #[arg(long, global = true, default_value = "~/.tokenforge/tokenforge.db")]
    db_path: String,
}

#[derive(Subcommand)]
enum Commands {
    /// Compress text/code/output for LLM consumption
    Compress {
        /// File path or '-' for stdin
        #[arg(default_value = "-")]
        input: String,

        /// Content type override
        #[arg(long, value_parser = parse_content_type)]
        r#type: Option<ContentType>,

        /// Language hint for code compression
        #[arg(long, value_parser = parse_language)]
        language: Option<Language>,

        /// Compression aggressiveness
        #[arg(long, default_value = "medium", value_parser = parse_level)]
        level: CompressionLevel,

        /// Target token count
        #[arg(long)]
        budget: Option<usize>,
    },

    /// PostToolUse hook mode (reads JSON from stdin)
    Hook {
        /// Session ID
        #[arg(long, default_value = "unknown")]
        session: String,

        /// Project directory
        #[arg(long)]
        project_dir: Option<PathBuf>,
    },

    /// Show compression statistics
    Stats {
        /// Session ID (default: all)
        #[arg(long, default_value = "current")]
        session: String,
    },

    /// Analyze token usage patterns
    Analyze {
        /// Session ID
        #[arg(long, default_value = "current")]
        session: String,
    },

    /// View or set per-category token budgets
    Budget {
        /// Total token budget
        #[arg(long)]
        set: Option<usize>,

        /// Show current budget
        #[arg(long)]
        show: bool,
    },

    /// Build project relevance profile
    Learn {
        /// Project directory
        #[arg(long, default_value = ".")]
        project: PathBuf,

        /// Reset learned profile
        #[arg(long)]
        reset: bool,
    },

    /// Show compression quality score
    Quality {
        /// Session ID
        #[arg(long, default_value = "current")]
        session: String,
    },

    /// Expand compressed content by hash
    Expand {
        /// Content hash
        hash: String,
    },

    /// Show diff between original and compressed content
    Diff {
        /// Content hash
        hash: String,
    },

    /// Run as MCP server over stdio (JSON-RPC 2.0)
    Serve,

    /// Auto-install PostToolUse hook into ~/.claude/settings.json
    Setup {
        /// Print what would change without writing
        #[arg(long)]
        dry_run: bool,
    },

    /// Benchmark all compression engines with real metrics
    Bench {
        /// Compression level to benchmark
        #[arg(long, default_value = "medium", value_parser = parse_level)]
        level: CompressionLevel,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let filter = if cli.verbose { "debug" } else { "warn" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(filter)),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    let db_path = resolve_db_path(&cli.db_path)?;

    match cli.command {
        Commands::Compress {
            input,
            r#type,
            language,
            level,
            budget: _,
        } => {
            let content = read_input(&input)?;
            let type_hint = r#type.or_else(|| {
                language.map(|l| ContentType::Code { language: l })
            });

            let engine = Engine::new(db_path).with_level(level);
            let result = engine.compress(&content, type_hint)?;

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                print!("{}", result.compressed);
                eprintln!(
                    "\n[tokenforge] {} → {} tokens ({:.0}% saved)",
                    result.original_tokens,
                    result.compressed_tokens,
                    result.ratio * 100.0
                );
            }
        }

        Commands::Hook { session, project_dir: _ } => {
            let mut stdin = String::new();
            std::io::stdin().read_to_string(&mut stdin)?;

            // Try to parse as HookInput JSON
            if let Ok(hook_input) = serde_json::from_str::<HookInput>(&stdin) {
                let engine = Engine::new(db_path);
                let result = engine.process_hook(&hook_input, &session)?;
                print!("{}", result.compressed);
                eprintln!(
                    "[tokenforge] {}: {} → {} tokens ({:.0}% saved)",
                    hook_input.tool_name,
                    result.original_tokens,
                    result.compressed_tokens,
                    result.ratio * 100.0,
                );
            } else {
                // Not valid hook JSON — pass through
                print!("{stdin}");
            }
        }

        Commands::Stats { session } => {
            let engine = Engine::new(db_path);
            let stats = engine.stats(&session)?;

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&stats)?);
            } else {
                println!("TokenForge Session Stats: {}", stats.session_id);
                println!("{}", "─".repeat(50));
                println!(
                    "  Total processed: {} tokens",
                    stats.total_original_tokens
                );
                println!(
                    "  After compression: {} tokens",
                    stats.total_compressed_tokens
                );
                println!(
                    "  Tokens saved: {} ({:.1}%)",
                    stats.tokens_saved,
                    stats.overall_ratio * 100.0
                );
                println!("  Compressions: {}", stats.compressions_count);
                println!();
                println!("  By type:");
                for ts in &stats.by_type {
                    println!(
                        "    {:<20} {:>6} → {:>6} ({:.0}% saved, {}x)",
                        ts.content_type,
                        ts.original_tokens,
                        ts.compressed_tokens,
                        ts.ratio * 100.0,
                        ts.count,
                    );
                }
            }
        }

        Commands::Analyze { session } => {
            let analysis =
                tokenforge::learning::patterns::analyze_session(&session, &db_path)?;

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&analysis)?);
            } else {
                println!("Session Analysis: {}", analysis.session_id);
                println!("{}", "─".repeat(50));
                println!("  Session type: {}", analysis.session_type);
                println!("  Tokens processed: {}", analysis.total_tokens_processed);
                println!(
                    "  Tokens saved: {} ({:.1}%)",
                    analysis.total_tokens_saved, analysis.savings_percentage,
                );
                if !analysis.recommendations.is_empty() {
                    println!();
                    println!("  Recommendations:");
                    for rec in &analysis.recommendations {
                        println!("    - {rec}");
                    }
                }
            }
        }

        Commands::Budget { set, show: _ } => {
            if let Some(total) = set {
                let budget = tokenforge::BudgetConfig {
                    total,
                    ..Default::default()
                };
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&budget)?);
                } else {
                    println!("Budget set: {} total tokens", budget.total);
                    println!("  Conversation: {:.0}%", budget.conversation * 100.0);
                    println!("  Tool output:  {:.0}%", budget.tool_output * 100.0);
                    println!("  Code context: {:.0}%", budget.code_context * 100.0);
                    println!("  MCP schemas:  {:.0}%", budget.mcp_schema * 100.0);
                }
            } else {
                let budget = tokenforge::BudgetConfig::default();
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&budget)?);
                } else {
                    println!("Current budget: {} total tokens", budget.total);
                    println!("  Conversation: {:.0}%", budget.conversation * 100.0);
                    println!("  Tool output:  {:.0}%", budget.tool_output * 100.0);
                    println!("  Code context: {:.0}%", budget.code_context * 100.0);
                    println!("  MCP schemas:  {:.0}%", budget.mcp_schema * 100.0);
                }
            }
        }

        Commands::Learn { project, reset: _ } => {
            let report =
                tokenforge::learning::profile::build_profile(&project, &db_path)?;

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("Project Profile: {}", report.project_path);
                println!("{}", "─".repeat(50));
                println!("  Git files indexed: {}", report.recent_git_files);
                println!("  Tracked files: {}", report.tracked_files);
                if !report.top_files.is_empty() {
                    println!();
                    println!("  Most accessed files:");
                    for f in &report.top_files {
                        println!("    {:>3}x  {}", f.access_count, f.path);
                    }
                }
            }
        }

        Commands::Quality { session } => {
            let report =
                tokenforge::quality::scorer::compute_quality_score(&session, &db_path)?;

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("Quality Report: {}", report.session_id);
                println!("{}", "─".repeat(50));
                println!("  Score: {:.0}/100", report.quality_score);
                println!("  Assessment: {}", report.assessment);
                println!("  Compressions analyzed: {}", report.compressions_analyzed);
                println!("  Average compression: {:.1}%", report.average_ratio * 100.0);
                if let Some(rec) = &report.recommendation {
                    println!("  Recommendation: {rec}");
                }
            }
        }

        Commands::Expand { hash } => {
            let engine = Engine::new(db_path);
            let original = engine.expand(&hash)?;
            print!("{original}");
        }

        Commands::Diff { hash } => {
            let engine = Engine::new(db_path);
            let result = engine.diff(&hash)?;

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("Diff for hash: {}", result.hash);
                println!("{}", "─".repeat(50));
                println!("  Type:       {}", result.content_type);
                println!(
                    "  Original:   {} bytes, {} tokens",
                    result.original_bytes, result.original_tokens
                );
                println!(
                    "  Compressed: {} bytes, {} tokens",
                    result.compressed_bytes, result.compressed_tokens
                );
                println!("  Savings:    {:.1}%", result.savings_pct);
                println!("{}", "─".repeat(50));
                print!("{}", result.unified_diff);
            }
        }

        Commands::Serve => {
            let server = mcp_server::McpServer::new(db_path);
            server.run()?;
        }

        Commands::Setup { dry_run } => {
            let report = setup::run_setup(dry_run)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", report.message);
                if !report.already_configured {
                    println!();
                    println!("  Settings: {}", report.settings_path);
                    println!("  Command:  {}", report.hook_command);
                }
            }
        }

        Commands::Bench { level } => {
            let results = bench::run_bench(&db_path, level)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&results)?);
            } else {
                print!("{}", bench::format_table(&results));
                let total_saved: usize = results.iter().map(|r| r.tokens_saved).sum();
                let avg_savings: f64 = results.iter().map(|r| r.savings_pct).sum::<f64>()
                    / results.len() as f64;
                println!(
                    "  Total tokens saved across all engines: {}  |  Avg savings: {:.1}%",
                    total_saved, avg_savings
                );
                println!();
            }
        }
    }

    Ok(())
}

fn resolve_db_path(raw: &str) -> Result<PathBuf> {
    let expanded = if raw.starts_with("~/") {
        let home = dirs_fallback();
        home.join(&raw[2..])
    } else {
        PathBuf::from(raw)
    };

    // Ensure parent directory exists
    if let Some(parent) = expanded.parent() {
        std::fs::create_dir_all(parent)?;
    }

    Ok(expanded)
}

fn dirs_fallback() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

fn read_input(input: &str) -> Result<String> {
    if input == "-" {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        Ok(buf)
    } else {
        std::fs::read_to_string(input)
            .map_err(|e| anyhow::anyhow!("failed to read {input}: {e}"))
    }
}

fn parse_content_type(s: &str) -> Result<ContentType, String> {
    match s.to_lowercase().as_str() {
        "code" => Ok(ContentType::Code {
            language: Language::Rust,
        }),
        "output" | "command_output" => Ok(ContentType::CommandOutput),
        "conversation" => Ok(ContentType::Conversation),
        "json" => Ok(ContentType::Json),
        "mcp" | "mcp_schema" => Ok(ContentType::McpSchema),
        _ => Err(format!("unknown type: {s}")),
    }
}

fn parse_language(s: &str) -> Result<Language, String> {
    Language::from_extension(s)
        .or_else(|| match s.to_lowercase().as_str() {
            "rust" => Some(Language::Rust),
            "typescript" => Some(Language::TypeScript),
            "javascript" => Some(Language::JavaScript),
            "python" => Some(Language::Python),
            "go" => Some(Language::Go),
            "java" => Some(Language::Java),
            "c" => Some(Language::C),
            "cpp" | "c++" => Some(Language::Cpp),
            _ => None,
        })
        .ok_or_else(|| format!("unknown language: {s}"))
}

fn parse_level(s: &str) -> Result<CompressionLevel, String> {
    match s.to_lowercase().as_str() {
        "light" | "l" => Ok(CompressionLevel::Light),
        "medium" | "m" => Ok(CompressionLevel::Medium),
        "aggressive" | "a" => Ok(CompressionLevel::Aggressive),
        _ => Err(format!("unknown level: {s} (expected: light, medium, aggressive)")),
    }
}
