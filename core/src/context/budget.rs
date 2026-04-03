use crate::{BudgetConfig, CompressionLevel};

/// Budget manager that determines compression aggressiveness per category.
pub struct BudgetManager {
    config: BudgetConfig,
    used: BudgetUsage,
}

#[derive(Debug, Default)]
pub struct BudgetUsage {
    pub conversation_tokens: usize,
    pub tool_output_tokens: usize,
    pub code_context_tokens: usize,
    pub mcp_schema_tokens: usize,
}

impl BudgetManager {
    pub fn new(config: BudgetConfig) -> Self {
        Self {
            config,
            used: BudgetUsage::default(),
        }
    }

    /// Get the token limit for a category.
    pub fn limit_for(&self, category: &str) -> usize {
        let fraction = match category {
            "conversation" => self.config.conversation,
            "tool_output" | "command_output" => self.config.tool_output,
            "code" | "code_context" => self.config.code_context,
            "mcp" | "mcp_schema" => self.config.mcp_schema,
            _ => 0.1,
        };
        (self.config.total as f64 * fraction as f64) as usize
    }

    /// Determine compression level based on budget pressure for a category.
    pub fn compression_level_for(&self, category: &str) -> CompressionLevel {
        let limit = self.limit_for(category);
        let used = self.used_for(category);
        let pressure = if limit > 0 {
            used as f64 / limit as f64
        } else {
            1.0
        };

        if pressure < 0.5 {
            CompressionLevel::Light
        } else if pressure < 0.8 {
            CompressionLevel::Medium
        } else {
            CompressionLevel::Aggressive
        }
    }

    /// Record token usage for a category.
    pub fn record_usage(&mut self, category: &str, tokens: usize) {
        match category {
            "conversation" => self.used.conversation_tokens += tokens,
            "tool_output" | "command_output" => self.used.tool_output_tokens += tokens,
            "code" | "code_context" => self.used.code_context_tokens += tokens,
            "mcp" | "mcp_schema" => self.used.mcp_schema_tokens += tokens,
            _ => {}
        }
    }

    fn used_for(&self, category: &str) -> usize {
        match category {
            "conversation" => self.used.conversation_tokens,
            "tool_output" | "command_output" => self.used.tool_output_tokens,
            "code" | "code_context" => self.used.code_context_tokens,
            "mcp" | "mcp_schema" => self.used.mcp_schema_tokens,
            _ => 0,
        }
    }

    /// Total tokens used across all categories.
    pub fn total_used(&self) -> usize {
        self.used.conversation_tokens
            + self.used.tool_output_tokens
            + self.used.code_context_tokens
            + self.used.mcp_schema_tokens
    }

    /// Remaining budget.
    pub fn remaining(&self) -> usize {
        self.config.total.saturating_sub(self.total_used())
    }
}
