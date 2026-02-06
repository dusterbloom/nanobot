//! Message tool for sending messages to users.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::Mutex;

use super::base::Tool;
use crate::bus::events::OutboundMessage;

/// Type alias for the send callback.
pub type SendCallback = Arc<
    dyn Fn(OutboundMessage) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send + Sync,
>;

/// Tool to send messages to users on chat channels.
pub struct MessageTool {
    send_callback: Arc<Mutex<Option<SendCallback>>>,
    default_channel: Arc<Mutex<String>>,
    default_chat_id: Arc<Mutex<String>>,
}

impl MessageTool {
    /// Create a new message tool.
    pub fn new(
        send_callback: Option<SendCallback>,
        default_channel: &str,
        default_chat_id: &str,
    ) -> Self {
        Self {
            send_callback: Arc::new(Mutex::new(send_callback)),
            default_channel: Arc::new(Mutex::new(default_channel.to_string())),
            default_chat_id: Arc::new(Mutex::new(default_chat_id.to_string())),
        }
    }

    /// Set the current message context.
    pub async fn set_context(&self, channel: &str, chat_id: &str) {
        *self.default_channel.lock().await = channel.to_string();
        *self.default_chat_id.lock().await = chat_id.to_string();
    }

    /// Set the callback for sending messages.
    pub async fn set_send_callback(&self, callback: SendCallback) {
        *self.send_callback.lock().await = Some(callback);
    }
}

#[async_trait]
impl Tool for MessageTool {
    fn name(&self) -> &str {
        "message"
    }

    fn description(&self) -> &str {
        "Send a message to the user. Use this when you want to communicate something."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "The message content to send"
                },
                "channel": {
                    "type": "string",
                    "description": "Optional: target channel (telegram, discord, etc.)"
                },
                "chat_id": {
                    "type": "string",
                    "description": "Optional: target chat/user ID"
                }
            },
            "required": ["content"]
        })
    }

    async fn execute(&self, params: HashMap<String, serde_json::Value>) -> String {
        let content = match params.get("content").and_then(|v| v.as_str()) {
            Some(c) => c.to_string(),
            None => return "Error: 'content' parameter is required".to_string(),
        };

        let default_channel = self.default_channel.lock().await.clone();
        let default_chat_id = self.default_chat_id.lock().await.clone();

        let channel = params
            .get("channel")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or(default_channel);

        let chat_id = params
            .get("chat_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or(default_chat_id);

        if channel.is_empty() || chat_id.is_empty() {
            return "Error: No target channel/chat specified".to_string();
        }

        let callback_guard = self.send_callback.lock().await;
        let callback = match callback_guard.as_ref() {
            Some(cb) => cb.clone(),
            None => return "Error: Message sending not configured".to_string(),
        };
        // Drop the lock before awaiting the callback.
        drop(callback_guard);

        let msg = OutboundMessage::new(&channel, &chat_id, &content);

        match callback(msg).await {
            Ok(()) => format!("Message sent to {}:{}", channel, chat_id),
            Err(e) => format!("Error sending message: {}", e),
        }
    }
}
