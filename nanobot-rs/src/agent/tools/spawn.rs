//! Spawn tool for creating background subagents.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use super::base::Tool;

/// Type alias for the spawn callback.
///
/// Arguments: (task, label, origin_channel, origin_chat_id) -> result string.
pub type SpawnCallback = Arc<
    dyn Fn(String, Option<String>, String, String) -> Pin<Box<dyn Future<Output = String> + Send>>
        + Send
        + Sync,
>;

/// Tool to spawn a subagent for background task execution.
///
/// The subagent runs asynchronously and announces its result back
/// to the main agent when complete.
pub struct SpawnTool {
    spawn_callback: Arc<Mutex<Option<SpawnCallback>>>,
    origin_channel: Arc<Mutex<String>>,
    origin_chat_id: Arc<Mutex<String>>,
}

impl SpawnTool {
    /// Create a new spawn tool.
    pub fn new() -> Self {
        Self {
            spawn_callback: Arc::new(Mutex::new(None)),
            origin_channel: Arc::new(Mutex::new("cli".to_string())),
            origin_chat_id: Arc::new(Mutex::new("direct".to_string())),
        }
    }

    /// Set the origin context for subagent announcements.
    pub async fn set_context(&self, channel: &str, chat_id: &str) {
        *self.origin_channel.lock().await = channel.to_string();
        *self.origin_chat_id.lock().await = chat_id.to_string();
    }

    /// Set the spawn callback.
    pub async fn set_callback(&self, callback: SpawnCallback) {
        *self.spawn_callback.lock().await = Some(callback);
    }
}

impl Default for SpawnTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for SpawnTool {
    fn name(&self) -> &str {
        "spawn"
    }

    fn description(&self) -> &str {
        "Spawn a subagent to handle a task in the background. \
         Use this for complex or time-consuming tasks that can run independently. \
         The subagent will complete the task and report back when done."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "The task for the subagent to complete"
                },
                "label": {
                    "type": "string",
                    "description": "Optional short label for the task (for display)"
                }
            },
            "required": ["task"]
        })
    }

    async fn execute(&self, params: HashMap<String, serde_json::Value>) -> String {
        let task = match params.get("task").and_then(|v| v.as_str()) {
            Some(t) => t.to_string(),
            None => return "Error: 'task' parameter is required".to_string(),
        };

        let label = params
            .get("label")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let channel = self.origin_channel.lock().await.clone();
        let chat_id = self.origin_chat_id.lock().await.clone();

        let callback_guard = self.spawn_callback.lock().await;
        let callback = match callback_guard.as_ref() {
            Some(cb) => cb.clone(),
            None => return "Error: Spawn callback not configured".to_string(),
        };
        // Drop the lock before awaiting.
        drop(callback_guard);

        callback(task, label, channel, chat_id).await
    }
}
