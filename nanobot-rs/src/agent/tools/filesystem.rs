//! File system tools: read, write, edit, list.

use std::collections::HashMap;
use std::path::PathBuf;

use async_trait::async_trait;

use super::base::Tool;

// ---------------------------------------------------------------------------
// ReadFileTool
// ---------------------------------------------------------------------------

/// Tool to read file contents.
pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file at the given path."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The file path to read"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, params: HashMap<String, serde_json::Value>) -> String {
        let path = match params.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return "Error: 'path' parameter is required".to_string(),
        };

        let file_path = expand_path(path);

        if !file_path.exists() {
            return format!("Error: File not found: {}", path);
        }
        if !file_path.is_file() {
            return format!("Error: Not a file: {}", path);
        }

        match tokio::fs::read_to_string(&file_path).await {
            Ok(content) => content,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    format!("Error: Permission denied: {}", path)
                } else {
                    format!("Error reading file: {}", e)
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// WriteFileTool
// ---------------------------------------------------------------------------

/// Tool to write content to a file.
pub struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to a file at the given path. Creates parent directories if needed."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The file path to write to"
                },
                "content": {
                    "type": "string",
                    "description": "The content to write"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, params: HashMap<String, serde_json::Value>) -> String {
        let path = match params.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return "Error: 'path' parameter is required".to_string(),
        };
        let content = match params.get("content").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return "Error: 'content' parameter is required".to_string(),
        };

        let file_path = expand_path(path);

        // Create parent directories.
        if let Some(parent) = file_path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return format!("Error creating directories: {}", e);
            }
        }

        match tokio::fs::write(&file_path, content).await {
            Ok(()) => format!("Successfully wrote {} bytes to {}", content.len(), path),
            Err(e) => {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    format!("Error: Permission denied: {}", path)
                } else {
                    format!("Error writing file: {}", e)
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// EditFileTool
// ---------------------------------------------------------------------------

/// Tool to edit a file by replacing text.
pub struct EditFileTool;

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Edit a file by replacing old_text with new_text. The old_text must exist exactly in the file."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The file path to edit"
                },
                "old_text": {
                    "type": "string",
                    "description": "The exact text to find and replace"
                },
                "new_text": {
                    "type": "string",
                    "description": "The text to replace with"
                }
            },
            "required": ["path", "old_text", "new_text"]
        })
    }

    async fn execute(&self, params: HashMap<String, serde_json::Value>) -> String {
        let path = match params.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return "Error: 'path' parameter is required".to_string(),
        };
        let old_text = match params.get("old_text").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => return "Error: 'old_text' parameter is required".to_string(),
        };
        let new_text = match params.get("new_text").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => return "Error: 'new_text' parameter is required".to_string(),
        };

        let file_path = expand_path(path);

        if !file_path.exists() {
            return format!("Error: File not found: {}", path);
        }

        let content = match tokio::fs::read_to_string(&file_path).await {
            Ok(c) => c,
            Err(e) => return format!("Error reading file: {}", e),
        };

        if !content.contains(old_text) {
            return "Error: old_text not found in file. Make sure it matches exactly.".to_string();
        }

        // Count occurrences.
        let count = content.matches(old_text).count();
        if count > 1 {
            return format!(
                "Warning: old_text appears {} times. Please provide more context to make it unique.",
                count
            );
        }

        let new_content = content.replacen(old_text, new_text, 1);

        match tokio::fs::write(&file_path, new_content).await {
            Ok(()) => format!("Successfully edited {}", path),
            Err(e) => {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    format!("Error: Permission denied: {}", path)
                } else {
                    format!("Error writing file: {}", e)
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ListDirTool
// ---------------------------------------------------------------------------

/// Tool to list directory contents.
pub struct ListDirTool;

#[async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "List the contents of a directory."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The directory path to list"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, params: HashMap<String, serde_json::Value>) -> String {
        let path = match params.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return "Error: 'path' parameter is required".to_string(),
        };

        let dir_path = expand_path(path);

        if !dir_path.exists() {
            return format!("Error: Directory not found: {}", path);
        }
        if !dir_path.is_dir() {
            return format!("Error: Not a directory: {}", path);
        }

        match tokio::fs::read_dir(&dir_path).await {
            Ok(mut entries) => {
                let mut items: Vec<(bool, String)> = Vec::new();

                loop {
                    match entries.next_entry().await {
                        Ok(Some(entry)) => {
                            let name = entry.file_name().to_string_lossy().to_string();
                            let is_dir = entry
                                .file_type()
                                .await
                                .map(|ft| ft.is_dir())
                                .unwrap_or(false);
                            items.push((is_dir, name));
                        }
                        Ok(None) => break,
                        Err(e) => return format!("Error reading directory: {}", e),
                    }
                }

                if items.is_empty() {
                    return format!("Directory {} is empty", path);
                }

                // Sort alphabetically.
                items.sort_by(|a, b| a.1.cmp(&b.1));

                let lines: Vec<String> = items
                    .into_iter()
                    .map(|(is_dir, name)| {
                        if is_dir {
                            format!("[dir]  {}", name)
                        } else {
                            format!("[file] {}", name)
                        }
                    })
                    .collect();

                lines.join("\n")
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    format!("Error: Permission denied: {}", path)
                } else {
                    format!("Error listing directory: {}", e)
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Expand a leading `~` to the user's home directory.
fn expand_path(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join(rest)
    } else if path == "~" {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
    } else {
        PathBuf::from(path)
    }
}
