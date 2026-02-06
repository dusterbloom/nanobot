//! Base class for agent tools.

use std::collections::HashMap;

use async_trait::async_trait;

/// Abstract base trait for agent tools.
///
/// Tools are capabilities that the agent can use to interact with
/// the environment, such as reading files, executing commands, etc.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Tool name used in function calls.
    fn name(&self) -> &str;

    /// Description of what the tool does.
    fn description(&self) -> &str;

    /// JSON Schema for tool parameters.
    fn parameters(&self) -> serde_json::Value;

    /// Execute the tool with given parameters.
    ///
    /// Returns the result as a string.
    async fn execute(&self, params: HashMap<String, serde_json::Value>) -> String;

    /// Convert tool to OpenAI function schema format.
    fn to_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name(),
                "description": self.description(),
                "parameters": self.parameters(),
            }
        })
    }
}
