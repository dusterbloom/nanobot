//! Tool registry for dynamic tool management.

use std::collections::HashMap;

use super::base::Tool;

/// Registry for agent tools.
///
/// Allows dynamic registration and execution of tools.
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    /// Create a new, empty registry.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool. Replaces any existing tool with the same name.
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.name().to_string();
        self.tools.insert(name, tool);
    }

    /// Unregister a tool by name.
    pub fn unregister(&mut self, name: &str) {
        self.tools.remove(name);
    }

    /// Get a reference to a tool by name.
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    /// Check if a tool is registered.
    pub fn has(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Get all tool definitions in OpenAI format.
    pub fn get_definitions(&self) -> Vec<serde_json::Value> {
        self.tools.values().map(|tool| tool.to_schema()).collect()
    }

    /// Execute a tool by name with given parameters.
    ///
    /// Returns the tool execution result as a string, or an error message
    /// if the tool is not found or execution fails.
    pub async fn execute(
        &self,
        name: &str,
        params: HashMap<String, serde_json::Value>,
    ) -> String {
        let tool = match self.tools.get(name) {
            Some(t) => t,
            None => return format!("Error: Tool '{}' not found", name),
        };

        match std::panic::AssertUnwindSafe(tool.execute(params))
            .await
        {
            result => result,
        }
    }

    /// Get list of registered tool names.
    pub fn tool_names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    /// Get the number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Check if a tool name is in the registry.
    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
