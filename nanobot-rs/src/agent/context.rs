//! Context builder for assembling agent prompts.
//!
//! Assembles bootstrap files, memory, skills, and conversation history into
//! a coherent prompt for the LLM.

use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use chrono::Local;
use serde_json::{json, Value};

use crate::agent::memory::MemoryStore;
use crate::agent::skills::SkillsLoader;

/// Well-known files that are loaded from the workspace root when present.
const BOOTSTRAP_FILES: &[&str] = &[
    "AGENTS.md",
    "SOUL.md",
    "USER.md",
    "TOOLS.md",
    "IDENTITY.md",
];

/// Builds the context (system prompt + messages) for the agent.
pub struct ContextBuilder {
    pub workspace: PathBuf,
    pub memory: MemoryStore,
    pub skills: SkillsLoader,
}

impl ContextBuilder {
    /// Create a new context builder for the given workspace.
    pub fn new(workspace: &Path) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
            memory: MemoryStore::new(workspace),
            skills: SkillsLoader::new(workspace, None),
        }
    }

    // ------------------------------------------------------------------
    // Public API
    // ------------------------------------------------------------------

    /// Build the system prompt from bootstrap files, memory, and skills.
    pub fn build_system_prompt(&self, skill_names: Option<&[String]>) -> String {
        let mut parts: Vec<String> = Vec::new();

        // Core identity.
        parts.push(self._get_identity());

        // Bootstrap files.
        let bootstrap = self._load_bootstrap_files();
        if !bootstrap.is_empty() {
            parts.push(bootstrap);
        }

        // Memory context.
        let memory = self.memory.get_memory_context();
        if !memory.is_empty() {
            parts.push(format!("# Memory\n\n{}", memory));
        }

        // Skills -- progressive loading:
        // 1. Always-loaded skills: full content included directly.
        let always_skills = self.skills.get_always_skills();
        if !always_skills.is_empty() {
            let always_content = self.skills.load_skills_for_context(&always_skills);
            if !always_content.is_empty() {
                parts.push(format!("# Active Skills\n\n{}", always_content));
            }
        }

        // 2. Available skills: summary only (agent can read_file for details).
        let skills_summary = self.skills.build_skills_summary();
        if !skills_summary.is_empty() {
            parts.push(format!(
                "# Skills\n\n\
                 The following skills extend your capabilities. \
                 To use a skill, read its SKILL.md file using the read_file tool.\n\
                 Skills with available=\"false\" need dependencies installed first \
                 - you can try installing them with apt/brew.\n\n\
                 {}",
                skills_summary
            ));
        }

        // 3. Explicitly requested skills.
        if let Some(names) = skill_names {
            if !names.is_empty() {
                let requested = self.skills.load_skills_for_context(names);
                if !requested.is_empty() {
                    parts.push(format!("# Requested Skills\n\n{}", requested));
                }
            }
        }

        parts.join("\n\n---\n\n")
    }

    /// Build the complete message list for an LLM call.
    pub fn build_messages(
        &self,
        history: &[Value],
        current_message: &str,
        skill_names: Option<&[String]>,
        media: Option<&[String]>,
        channel: Option<&str>,
        chat_id: Option<&str>,
    ) -> Vec<Value> {
        let mut messages: Vec<Value> = Vec::new();

        // System prompt.
        let mut system_prompt = self.build_system_prompt(skill_names);
        if let (Some(ch), Some(cid)) = (channel, chat_id) {
            system_prompt
                .push_str(&format!("\n\n## Current Session\nChannel: {}\nChat ID: {}", ch, cid));
        }
        messages.push(json!({"role": "system", "content": system_prompt}));

        // History.
        messages.extend(history.iter().cloned());

        // Current user message (with optional image attachments).
        let user_content = Self::_build_user_content(current_message, media);
        messages.push(json!({"role": "user", "content": user_content}));

        messages
    }

    /// Add a tool result to the message list and return the updated list.
    pub fn add_tool_result(
        messages: &mut Vec<Value>,
        tool_call_id: &str,
        tool_name: &str,
        result: &str,
    ) {
        messages.push(json!({
            "role": "tool",
            "tool_call_id": tool_call_id,
            "name": tool_name,
            "content": result,
        }));
    }

    /// Add an assistant message (possibly with tool calls) to the message list.
    pub fn add_assistant_message(
        messages: &mut Vec<Value>,
        content: Option<&str>,
        tool_calls: Option<&[Value]>,
    ) {
        let mut msg = json!({
            "role": "assistant",
            "content": content.unwrap_or(""),
        });

        if let Some(tc) = tool_calls {
            if !tc.is_empty() {
                msg["tool_calls"] = Value::Array(tc.to_vec());
            }
        }

        messages.push(msg);
    }

    // ------------------------------------------------------------------
    // Private helpers
    // ------------------------------------------------------------------

    /// Core identity section including current time and workspace info.
    fn _get_identity(&self) -> String {
        let now = Local::now().format("%Y-%m-%d %H:%M (%A)").to_string();
        let workspace_path = self
            .workspace
            .canonicalize()
            .unwrap_or_else(|_| self.workspace.clone())
            .to_string_lossy()
            .to_string();

        format!(
            r#"# nanobot

You are nanobot, a helpful AI assistant. You have access to tools that allow you to:
- Read, write, and edit files
- Execute shell commands
- Search the web and fetch web pages
- Send messages to users on chat channels
- Spawn subagents for complex background tasks

## Current Time
{now}

## Workspace
Your workspace is at: {workspace_path}
- Memory files: {workspace_path}/memory/MEMORY.md
- Daily notes: {workspace_path}/memory/YYYY-MM-DD.md
- Custom skills: {workspace_path}/skills/{{skill-name}}/SKILL.md

IMPORTANT: When responding to direct questions or conversations, reply directly with your text response.
Only use the 'message' tool when you need to send a message to a specific chat channel (like WhatsApp).
For normal conversation, just respond with text - do not call the message tool.

Always be helpful, accurate, and concise. When using tools, explain what you're doing.
When remembering something, write to {workspace_path}/memory/MEMORY.md"#
        )
    }

    /// Load all bootstrap files from workspace.
    fn _load_bootstrap_files(&self) -> String {
        let mut parts: Vec<String> = Vec::new();

        for filename in BOOTSTRAP_FILES {
            let file_path = self.workspace.join(filename);
            if file_path.exists() {
                if let Ok(content) = fs::read_to_string(&file_path) {
                    parts.push(format!("## {}\n\n{}", filename, content));
                }
            }
        }

        parts.join("\n\n")
    }

    /// Build user message content with optional base64-encoded images.
    ///
    /// If media contains image files, returns a JSON array of content parts.
    /// Otherwise returns a plain string value.
    fn _build_user_content(text: &str, media: Option<&[String]>) -> Value {
        let media = match media {
            Some(m) if !m.is_empty() => m,
            _ => return Value::String(text.to_string()),
        };

        let mut images: Vec<Value> = Vec::new();

        for path_str in media {
            let path = Path::new(path_str);
            if !path.is_file() {
                continue;
            }
            let mime = _guess_mime(path_str);
            if !mime.starts_with("image/") {
                continue;
            }
            if let Ok(bytes) = fs::read(path) {
                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                images.push(json!({
                    "type": "image_url",
                    "image_url": {
                        "url": format!("data:{};base64,{}", mime, b64),
                    }
                }));
            }
        }

        if images.is_empty() {
            return Value::String(text.to_string());
        }

        // Append text part after images.
        images.push(json!({"type": "text", "text": text}));
        Value::Array(images)
    }
}

/// Guess MIME type from a file extension.
fn _guess_mime(path: &str) -> String {
    let lower = path.to_lowercase();
    if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg".to_string()
    } else if lower.ends_with(".png") {
        "image/png".to_string()
    } else if lower.ends_with(".gif") {
        "image/gif".to_string()
    } else if lower.ends_with(".webp") {
        "image/webp".to_string()
    } else if lower.ends_with(".svg") {
        "image/svg+xml".to_string()
    } else {
        "application/octet-stream".to_string()
    }
}
