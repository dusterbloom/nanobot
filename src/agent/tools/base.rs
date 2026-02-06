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

#[cfg(test)]
mod tests {
    use super::*;

    /// A mock tool for testing the Tool trait and to_schema().
    struct MockTool;

    #[async_trait]
    impl Tool for MockTool {
        fn name(&self) -> &str {
            "mock_tool"
        }

        fn description(&self) -> &str {
            "A mock tool for testing"
        }

        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "input": {
                        "type": "string",
                        "description": "Test input"
                    }
                },
                "required": ["input"]
            })
        }

        async fn execute(&self, params: HashMap<String, serde_json::Value>) -> String {
            let input = params
                .get("input")
                .and_then(|v| v.as_str())
                .unwrap_or("none");
            format!("executed with: {}", input)
        }
    }

    #[test]
    fn test_mock_tool_name() {
        let tool = MockTool;
        assert_eq!(tool.name(), "mock_tool");
    }

    #[test]
    fn test_mock_tool_description() {
        let tool = MockTool;
        assert_eq!(tool.description(), "A mock tool for testing");
    }

    #[test]
    fn test_mock_tool_parameters() {
        let tool = MockTool;
        let params = tool.parameters();
        assert_eq!(params["type"], "object");
        assert!(params["properties"]["input"].is_object());
        let required = params["required"].as_array().unwrap();
        assert_eq!(required.len(), 1);
        assert_eq!(required[0], "input");
    }

    #[test]
    fn test_to_schema_structure() {
        let tool = MockTool;
        let schema = tool.to_schema();

        assert_eq!(schema["type"], "function");
        assert!(schema["function"].is_object());
        assert_eq!(schema["function"]["name"], "mock_tool");
        assert_eq!(schema["function"]["description"], "A mock tool for testing");
        assert_eq!(schema["function"]["parameters"]["type"], "object");
    }

    #[test]
    fn test_to_schema_contains_all_fields() {
        let tool = MockTool;
        let schema = tool.to_schema();
        let function = &schema["function"];

        // Verify all expected keys are present.
        assert!(function.get("name").is_some());
        assert!(function.get("description").is_some());
        assert!(function.get("parameters").is_some());
    }

    #[tokio::test]
    async fn test_mock_tool_execute() {
        let tool = MockTool;
        let mut params = HashMap::new();
        params.insert(
            "input".to_string(),
            serde_json::Value::String("hello".to_string()),
        );
        let result = tool.execute(params).await;
        assert_eq!(result, "executed with: hello");
    }

    #[tokio::test]
    async fn test_mock_tool_execute_missing_param() {
        let tool = MockTool;
        let params = HashMap::new();
        let result = tool.execute(params).await;
        assert_eq!(result, "executed with: none");
    }
}
