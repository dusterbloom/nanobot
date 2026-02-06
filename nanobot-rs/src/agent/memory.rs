//! Memory system for persistent agent memory.
//!
//! Supports daily notes (`memory/YYYY-MM-DD.md`) and long-term memory (`MEMORY.md`).

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{Local, NaiveDate};

use crate::utils::helpers::{ensure_dir, today_date};

/// Persistent memory store for the agent.
pub struct MemoryStore {
    /// Root workspace path.
    pub workspace: PathBuf,
    /// Directory that contains all memory files.
    pub memory_dir: PathBuf,
    /// Path to the long-term memory file.
    pub memory_file: PathBuf,
}

impl MemoryStore {
    /// Create a new `MemoryStore` for the given workspace.
    pub fn new(workspace: &Path) -> Self {
        let memory_dir = ensure_dir(workspace.join("memory"));
        let memory_file = memory_dir.join("MEMORY.md");
        Self {
            workspace: workspace.to_path_buf(),
            memory_dir,
            memory_file,
        }
    }

    /// Get path to today's memory file.
    pub fn get_today_file(&self) -> PathBuf {
        self.memory_dir.join(format!("{}.md", today_date()))
    }

    /// Read today's memory notes. Returns empty string if no file exists.
    pub fn read_today(&self) -> String {
        let today_file = self.get_today_file();
        if today_file.exists() {
            fs::read_to_string(&today_file).unwrap_or_default()
        } else {
            String::new()
        }
    }

    /// Append content to today's memory notes.
    ///
    /// Creates the file with a date header if it does not exist yet.
    pub fn append_today(&self, content: &str) {
        let today_file = self.get_today_file();

        let full_content = if today_file.exists() {
            let existing = fs::read_to_string(&today_file).unwrap_or_default();
            format!("{}\n{}", existing, content)
        } else {
            let header = format!("# {}\n\n", today_date());
            format!("{}{}", header, content)
        };

        let _ = fs::write(&today_file, full_content);
    }

    /// Read long-term memory (`MEMORY.md`).
    pub fn read_long_term(&self) -> String {
        if self.memory_file.exists() {
            fs::read_to_string(&self.memory_file).unwrap_or_default()
        } else {
            String::new()
        }
    }

    /// Write to long-term memory (`MEMORY.md`), replacing existing content.
    pub fn write_long_term(&self, content: &str) {
        let _ = fs::write(&self.memory_file, content);
    }

    /// Get memories from the last N days, concatenated with separators.
    pub fn get_recent_memories(&self, days: u32) -> String {
        let today = Local::now().date_naive();
        let mut memories: Vec<String> = Vec::new();

        for i in 0..days {
            let date = today - chrono::Duration::days(i64::from(i));
            let date_str = date.format("%Y-%m-%d").to_string();
            let file_path = self.memory_dir.join(format!("{}.md", date_str));

            if file_path.exists() {
                if let Ok(content) = fs::read_to_string(&file_path) {
                    memories.push(content);
                }
            }
        }

        memories.join("\n\n---\n\n")
    }

    /// List all memory files sorted by date (newest first).
    pub fn list_memory_files(&self) -> Vec<PathBuf> {
        if !self.memory_dir.exists() {
            return Vec::new();
        }

        let mut files: Vec<PathBuf> = Vec::new();

        if let Ok(entries) = fs::read_dir(&self.memory_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    // Match YYYY-MM-DD.md pattern.
                    if name.len() == 13
                        && name.ends_with(".md")
                        && NaiveDate::parse_from_str(&name[..10], "%Y-%m-%d").is_ok()
                    {
                        files.push(path);
                    }
                }
            }
        }

        files.sort_by(|a, b| b.cmp(a));
        files
    }

    /// Get memory context for the agent prompt.
    ///
    /// Combines long-term memory and today's notes.
    pub fn get_memory_context(&self) -> String {
        let mut parts: Vec<String> = Vec::new();

        let long_term = self.read_long_term();
        if !long_term.is_empty() {
            parts.push(format!("## Long-term Memory\n{}", long_term));
        }

        let today = self.read_today();
        if !today.is_empty() {
            parts.push(format!("## Today's Notes\n{}", today));
        }

        if parts.is_empty() {
            String::new()
        } else {
            parts.join("\n\n")
        }
    }
}
