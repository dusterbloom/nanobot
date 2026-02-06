//! Base LLM provider interface.

use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A tool call request from the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRequest {
    pub id: String,
    pub name: String,
    pub arguments: HashMap<String, serde_json::Value>,
}

/// Response from an LLM provider.
#[derive(Debug, Clone)]
pub struct LLMResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCallRequest>,
    pub finish_reason: String,
    pub usage: HashMap<String, i64>,
}

impl LLMResponse {
    /// Check if response contains tool calls.
    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }
}

/// Abstract base trait for LLM providers.
///
/// Implementations should handle the specifics of each provider's API
/// while maintaining a consistent interface.
#[async_trait]
pub trait LLMProvider: Send + Sync {
    /// Send a chat completion request.
    ///
    /// # Arguments
    /// * `messages` - List of message objects with `role` and `content`.
    /// * `tools` - Optional list of tool definitions in OpenAI format.
    /// * `model` - Model identifier (provider-specific).
    /// * `max_tokens` - Maximum tokens in response.
    /// * `temperature` - Sampling temperature.
    async fn chat(
        &self,
        messages: &[serde_json::Value],
        tools: Option<&[serde_json::Value]>,
        model: Option<&str>,
        max_tokens: u32,
        temperature: f64,
    ) -> Result<LLMResponse>;

    /// Get the default model for this provider.
    fn get_default_model(&self) -> &str;
}
