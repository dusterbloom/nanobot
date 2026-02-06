//! Shell execution tool.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use regex::Regex;
use tokio::process::Command;

use super::base::Tool;

/// Default deny patterns for dangerous shell commands.
fn default_deny_patterns() -> Vec<String> {
    vec![
        r"\brm\s+-[rf]{1,2}\b".to_string(),
        r"\bdel\s+/[fq]\b".to_string(),
        r"\brmdir\s+/s\b".to_string(),
        r"\b(format|mkfs|diskpart)\b".to_string(),
        r"\bdd\s+if=".to_string(),
        r">\s*/dev/sd".to_string(),
        r"\b(shutdown|reboot|poweroff)\b".to_string(),
        r":\(\)\s*\{.*\};\s*:".to_string(),
    ]
}

/// Tool to execute shell commands.
pub struct ExecTool {
    timeout: u64,
    working_dir: Option<String>,
    deny_patterns: Vec<String>,
    allow_patterns: Vec<String>,
    restrict_to_workspace: bool,
}

impl ExecTool {
    /// Create a new `ExecTool`.
    pub fn new(
        timeout: u64,
        working_dir: Option<String>,
        deny_patterns: Option<Vec<String>>,
        allow_patterns: Option<Vec<String>>,
        restrict_to_workspace: bool,
    ) -> Self {
        Self {
            timeout,
            working_dir,
            deny_patterns: deny_patterns.unwrap_or_else(default_deny_patterns),
            allow_patterns: allow_patterns.unwrap_or_default(),
            restrict_to_workspace,
        }
    }

    /// Best-effort safety guard for potentially destructive commands.
    ///
    /// Returns an error message if the command is blocked, or `None` if allowed.
    fn guard_command(&self, command: &str, cwd: &str) -> Option<String> {
        let cmd = command.trim();
        let lower = cmd.to_lowercase();

        // Check deny patterns.
        for pattern in &self.deny_patterns {
            if let Ok(re) = Regex::new(pattern) {
                if re.is_match(&lower) {
                    return Some(
                        "Error: Command blocked by safety guard (dangerous pattern detected)"
                            .to_string(),
                    );
                }
            }
        }

        // Check allow patterns (if any are configured, command must match at least one).
        if !self.allow_patterns.is_empty() {
            let allowed = self.allow_patterns.iter().any(|pattern| {
                Regex::new(pattern)
                    .map(|re| re.is_match(&lower))
                    .unwrap_or(false)
            });
            if !allowed {
                return Some(
                    "Error: Command blocked by safety guard (not in allowlist)".to_string(),
                );
            }
        }

        // Workspace restriction checks.
        if self.restrict_to_workspace {
            if cmd.contains("../") || cmd.contains("..\\") {
                return Some(
                    "Error: Command blocked by safety guard (path traversal detected)".to_string(),
                );
            }

            let cwd_path = match Path::new(cwd).canonicalize() {
                Ok(p) => p,
                Err(_) => PathBuf::from(cwd),
            };

            // Extract absolute paths from the command.
            let posix_re = Regex::new(r#"/[^\s"']+"#).unwrap_or_else(|_| Regex::new(r"^$").unwrap());
            let win_re =
                Regex::new(r#"[A-Za-z]:\\[^\\"']+"#).unwrap_or_else(|_| Regex::new(r"^$").unwrap());

            let mut paths: Vec<String> = Vec::new();
            for m in posix_re.find_iter(cmd) {
                paths.push(m.as_str().to_string());
            }
            for m in win_re.find_iter(cmd) {
                paths.push(m.as_str().to_string());
            }

            for raw in paths {
                if let Ok(p) = Path::new(&raw).canonicalize() {
                    if p != cwd_path && !p.starts_with(&cwd_path) {
                        return Some(
                            "Error: Command blocked by safety guard (path outside working dir)"
                                .to_string(),
                        );
                    }
                }
            }
        }

        None
    }
}

#[async_trait]
impl Tool for ExecTool {
    fn name(&self) -> &str {
        "exec"
    }

    fn description(&self) -> &str {
        "Execute a shell command and return its output. Use with caution."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "working_dir": {
                    "type": "string",
                    "description": "Optional working directory for the command"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, params: HashMap<String, serde_json::Value>) -> String {
        let command = match params.get("command").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return "Error: 'command' parameter is required".to_string(),
        };

        let param_cwd = params
            .get("working_dir")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let cwd = param_cwd
            .or_else(|| self.working_dir.clone())
            .unwrap_or_else(|| {
                std::env::current_dir()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| ".".to_string())
            });

        // Safety guard.
        if let Some(error) = self.guard_command(command, &cwd) {
            return error;
        }

        let result = tokio::time::timeout(
            Duration::from_secs(self.timeout),
            async {
                let output = Command::new("sh")
                    .arg("-c")
                    .arg(command)
                    .current_dir(&cwd)
                    .output()
                    .await;

                match output {
                    Ok(output) => {
                        let mut parts: Vec<String> = Vec::new();

                        let stdout = String::from_utf8_lossy(&output.stdout);
                        if !stdout.is_empty() {
                            parts.push(stdout.to_string());
                        }

                        let stderr = String::from_utf8_lossy(&output.stderr);
                        if !stderr.trim().is_empty() {
                            parts.push(format!("STDERR:\n{}", stderr));
                        }

                        if !output.status.success() {
                            let code = output.status.code().unwrap_or(-1);
                            parts.push(format!("\nExit code: {}", code));
                        }

                        if parts.is_empty() {
                            "(no output)".to_string()
                        } else {
                            parts.join("\n")
                        }
                    }
                    Err(e) => format!("Error executing command: {}", e),
                }
            },
        )
        .await;

        let mut output = match result {
            Ok(s) => s,
            Err(_) => {
                format!("Error: Command timed out after {} seconds", self.timeout)
            }
        };

        // Truncate very long output.
        let max_len = 10000;
        if output.len() > max_len {
            let overflow = output.len() - max_len;
            output.truncate(max_len);
            output.push_str(&format!("\n... (truncated, {} more chars)", overflow));
        }

        output
    }
}
