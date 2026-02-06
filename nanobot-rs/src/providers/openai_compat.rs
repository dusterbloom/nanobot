//! OpenAI-compatible API provider.
//!
//! Replaces LiteLLMProvider by calling OpenAI-compatible APIs directly via reqwest.
//! Supports OpenRouter, Anthropic (OpenAI-compat endpoint), OpenAI, DeepSeek,
//! Groq, vLLM, and any other provider that implements the OpenAI chat completions
//! API format.

use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use tracing::warn;

use super::base::{LLMProvider, LLMResponse, ToolCallRequest};

/// An LLM provider that talks to any OpenAI-compatible chat completions endpoint.
pub struct OpenAICompatProvider {
    api_key: String,
    api_base: String,
    default_model: String,
    client: Client,
}

impl OpenAICompatProvider {
    /// Create a new provider.
    ///
    /// Provider detection logic (porting from `LiteLLMProvider.__init__`):
    /// - OpenRouter: detected by `sk-or-` key prefix or `openrouter` in api_base
    /// - DeepSeek: detected by `deepseek` in the default model name
    /// - vLLM / custom: when an explicit `api_base` is provided that isn't OpenRouter
    /// - Default fallback: OpenRouter (`https://openrouter.ai/api/v1`)
    pub fn new(
        api_key: &str,
        api_base: Option<&str>,
        default_model: Option<&str>,
    ) -> Self {
        let default_model = default_model
            .unwrap_or("anthropic/claude-opus-4-5")
            .to_string();

        let is_openrouter = api_key.starts_with("sk-or-")
            || api_base
                .map(|b| b.contains("openrouter"))
                .unwrap_or(false);

        let resolved_base = if let Some(base) = api_base {
            // Use whatever was explicitly provided.
            base.trim_end_matches('/').to_string()
        } else if is_openrouter {
            "https://openrouter.ai/api/v1".to_string()
        } else if default_model.contains("deepseek") {
            "https://api.deepseek.com".to_string()
        } else if default_model.contains("groq") {
            "https://api.groq.com/openai/v1".to_string()
        } else {
            // Sensible default: OpenRouter.
            "https://openrouter.ai/api/v1".to_string()
        };

        Self {
            api_key: api_key.to_string(),
            api_base: resolved_base,
            default_model,
            client: Client::new(),
        }
    }
}

#[async_trait]
impl LLMProvider for OpenAICompatProvider {
    async fn chat(
        &self,
        messages: &[serde_json::Value],
        tools: Option<&[serde_json::Value]>,
        model: Option<&str>,
        max_tokens: u32,
        temperature: f64,
    ) -> Result<LLMResponse> {
        let model = model.unwrap_or(&self.default_model);
        let url = format!("{}/chat/completions", self.api_base);

        // Build request body.
        let mut body = serde_json::json!({
            "model": model,
            "messages": messages,
            "max_tokens": max_tokens,
            "temperature": temperature,
        });

        if let Some(tool_defs) = tools {
            if !tool_defs.is_empty() {
                body["tools"] = serde_json::Value::Array(tool_defs.to_vec());
                body["tool_choice"] = serde_json::json!("auto");
            }
        }

        let response = match self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!("HTTP request to LLM failed: {}", e);
                return Ok(LLMResponse {
                    content: Some(format!("Error calling LLM: {}", e)),
                    tool_calls: Vec::new(),
                    finish_reason: "error".to_string(),
                    usage: HashMap::new(),
                });
            }
        };

        let status = response.status();
        let response_text = match response.text().await {
            Ok(t) => t,
            Err(e) => {
                return Ok(LLMResponse {
                    content: Some(format!("Error reading LLM response: {}", e)),
                    tool_calls: Vec::new(),
                    finish_reason: "error".to_string(),
                    usage: HashMap::new(),
                });
            }
        };

        if !status.is_success() {
            warn!("LLM API returned status {}: {}", status, response_text);
            return Ok(LLMResponse {
                content: Some(format!(
                    "Error calling LLM (HTTP {}): {}",
                    status, response_text
                )),
                tool_calls: Vec::new(),
                finish_reason: "error".to_string(),
                usage: HashMap::new(),
            });
        }

        let data: serde_json::Value = match serde_json::from_str(&response_text) {
            Ok(v) => v,
            Err(e) => {
                return Ok(LLMResponse {
                    content: Some(format!("Error parsing LLM response JSON: {}", e)),
                    tool_calls: Vec::new(),
                    finish_reason: "error".to_string(),
                    usage: HashMap::new(),
                });
            }
        };

        parse_response(&data)
    }

    fn get_default_model(&self) -> &str {
        &self.default_model
    }
}

/// Parse the OpenAI-compatible JSON response into an `LLMResponse`.
fn parse_response(data: &serde_json::Value) -> Result<LLMResponse> {
    let choices = data
        .get("choices")
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();

    if choices.is_empty() {
        return Ok(LLMResponse {
            content: Some("Error: No choices in LLM response".to_string()),
            tool_calls: Vec::new(),
            finish_reason: "error".to_string(),
            usage: HashMap::new(),
        });
    }

    let choice = &choices[0];
    let message = choice.get("message").cloned().unwrap_or_default();
    let finish_reason = choice
        .get("finish_reason")
        .and_then(|v| v.as_str())
        .unwrap_or("stop")
        .to_string();

    // Extract content.
    let content = message
        .get("content")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Extract tool calls.
    let mut tool_calls = Vec::new();
    if let Some(tc_array) = message.get("tool_calls").and_then(|v| v.as_array()) {
        for tc in tc_array {
            let id = tc
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let function = tc.get("function").cloned().unwrap_or_default();
            let name = function
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // Arguments come as a JSON string that we need to parse.
            let arguments_raw = function
                .get("arguments")
                .cloned()
                .unwrap_or(serde_json::Value::String("{}".to_string()));

            let arguments: HashMap<String, serde_json::Value> = if let Some(s) =
                arguments_raw.as_str()
            {
                match serde_json::from_str(s) {
                    Ok(map) => map,
                    Err(_) => {
                        let mut m = HashMap::new();
                        m.insert("raw".to_string(), serde_json::Value::String(s.to_string()));
                        m
                    }
                }
            } else if let Some(obj) = arguments_raw.as_object() {
                obj.iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            } else {
                HashMap::new()
            };

            tool_calls.push(ToolCallRequest {
                id,
                name,
                arguments,
            });
        }
    }

    // Extract usage.
    let mut usage = HashMap::new();
    if let Some(usage_obj) = data.get("usage").and_then(|v| v.as_object()) {
        for (key, value) in usage_obj {
            if let Some(n) = value.as_i64() {
                usage.insert(key.clone(), n);
            }
        }
    }

    Ok(LLMResponse {
        content,
        tool_calls,
        finish_reason,
        usage,
    })
}
