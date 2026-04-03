use crate::{BudgetConfig, CompressResult, ContentType, Engine};
use std::path::PathBuf;

use super::budget::BudgetManager;
use super::store::Store;

/// Context orchestrator — manages working set, budgets, and expansion.
pub struct ContextManager {
    store: Store,
    budget: BudgetManager,
    db_path: PathBuf,
}

impl ContextManager {
    pub fn new(db_path: PathBuf, budget_config: BudgetConfig) -> anyhow::Result<Self> {
        let store = Store::open(&db_path)?;
        let budget = BudgetManager::new(budget_config);
        Ok(Self {
            store,
            budget,
            db_path,
        })
    }

    /// Compress content with budget-aware compression level.
    pub fn compress_with_budget(
        &mut self,
        content: &str,
        content_type: &ContentType,
    ) -> anyhow::Result<CompressResult> {
        let category = match content_type {
            ContentType::Code { .. } => "code",
            ContentType::CommandOutput => "tool_output",
            ContentType::Conversation => "conversation",
            ContentType::Json => "tool_output",
            ContentType::McpSchema => "mcp",
            ContentType::Unknown => "tool_output",
        };

        let level = self.budget.compression_level_for(category);
        let engine = Engine::new(self.db_path.clone()).with_level(level);
        let result = engine.compress(content, Some(content_type.clone()))?;

        self.budget.record_usage(category, result.compressed_tokens);

        Ok(result)
    }

    /// Get remaining token budget.
    pub fn remaining_budget(&self) -> usize {
        self.budget.remaining()
    }

    /// Expand compressed content by hash.
    pub fn expand(&self, hash: &str) -> anyhow::Result<String> {
        self.store.get_original(hash)
    }
}
