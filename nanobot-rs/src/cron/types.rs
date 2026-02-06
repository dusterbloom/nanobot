//! Cron types – schedule definitions, payloads, job state, and persistence.

use serde::{Deserialize, Serialize};

/// Schedule definition for a cron job.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronSchedule {
    /// One of `"at"`, `"every"`, or `"cron"`.
    pub kind: String,
    /// For `"at"`: absolute timestamp in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at_ms: Option<i64>,
    /// For `"every"`: interval in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub every_ms: Option<i64>,
    /// For `"cron"`: a cron expression (e.g. `"0 9 * * *"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expr: Option<String>,
    /// Timezone for cron expressions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tz: Option<String>,
}

impl Default for CronSchedule {
    fn default() -> Self {
        Self {
            kind: "every".to_string(),
            at_ms: None,
            every_ms: None,
            expr: None,
            tz: None,
        }
    }
}

/// What to do when the job runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronPayload {
    /// `"system_event"` or `"agent_turn"`.
    #[serde(default = "default_payload_kind")]
    pub kind: String,
    /// The message/prompt to send.
    #[serde(default)]
    pub message: String,
    /// Whether to deliver the response to a channel.
    #[serde(default)]
    pub deliver: bool,
    /// Target channel name (e.g. `"whatsapp"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    /// Recipient identifier (e.g. a phone number).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
}

fn default_payload_kind() -> String {
    "agent_turn".to_string()
}

impl Default for CronPayload {
    fn default() -> Self {
        Self {
            kind: default_payload_kind(),
            message: String::new(),
            deliver: false,
            channel: None,
            to: None,
        }
    }
}

/// Runtime state of a job.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronJobState {
    /// Next scheduled run time in milliseconds since epoch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_run_at_ms: Option<i64>,
    /// Last completed run time in milliseconds since epoch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run_at_ms: Option<i64>,
    /// `"ok"`, `"error"`, or `"skipped"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_status: Option<String>,
    /// Error message from the last run, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// A scheduled job.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronJob {
    pub id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub schedule: CronSchedule,
    #[serde(default)]
    pub payload: CronPayload,
    #[serde(default)]
    pub state: CronJobState,
    #[serde(default)]
    pub created_at_ms: i64,
    #[serde(default)]
    pub updated_at_ms: i64,
    #[serde(default)]
    pub delete_after_run: bool,
}

fn default_true() -> bool {
    true
}

/// Persistent store for cron jobs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronStore {
    #[serde(default = "default_version")]
    pub version: i32,
    #[serde(default)]
    pub jobs: Vec<CronJob>,
}

fn default_version() -> i32 {
    1
}

impl Default for CronStore {
    fn default() -> Self {
        Self {
            version: 1,
            jobs: Vec::new(),
        }
    }
}
