//! Cron service for managing scheduled jobs.

use std::path::PathBuf;

use chrono::Local;
use tracing::{info, warn};
use uuid::Uuid;

use crate::cron::types::{CronJob, CronJobState, CronPayload, CronSchedule, CronStore};

fn now_ms() -> i64 {
    Local::now().timestamp_millis()
}

/// Service that manages cron jobs with file-based persistence.
pub struct CronService {
    store_path: PathBuf,
    store: CronStore,
    running: bool,
}

impl CronService {
    /// Create a new `CronService` with the given store file path.
    pub fn new(store_path: PathBuf) -> Self {
        let store = if store_path.exists() {
            std::fs::read_to_string(&store_path)
                .ok()
                .and_then(|c| serde_json::from_str(&c).ok())
                .unwrap_or_default()
        } else {
            CronStore::default()
        };
        Self {
            store_path,
            store,
            running: false,
        }
    }

    /// Start the cron service.
    pub async fn start(&mut self) {
        self.running = true;
        info!(
            "Cron service started with {} jobs",
            self.store.jobs.len()
        );
    }

    /// Stop the cron service.
    pub fn stop(&mut self) {
        self.running = false;
    }

    /// Add a new cron job and persist the store.
    pub fn add_job(
        &mut self,
        name: &str,
        schedule: CronSchedule,
        message: &str,
        deliver: bool,
        channel: Option<&str>,
        to: Option<&str>,
        delete_after_run: bool,
    ) -> CronJob {
        let now = now_ms();
        let id = Uuid::new_v4().to_string();
        let short_id = id[..8].to_string();

        let job = CronJob {
            id: short_id,
            name: name.to_string(),
            enabled: true,
            schedule,
            payload: CronPayload {
                kind: "agent_turn".to_string(),
                message: message.to_string(),
                deliver,
                channel: channel.map(|s| s.to_string()),
                to: to.map(|s| s.to_string()),
            },
            state: CronJobState::default(),
            created_at_ms: now,
            updated_at_ms: now,
            delete_after_run,
        };

        self.store.jobs.push(job.clone());
        self.persist();
        info!("Cron: added job '{}' ({})", job.name, job.id);
        job
    }

    /// List all registered jobs.
    pub fn list_jobs(&self, include_disabled: bool) -> Vec<CronJob> {
        if include_disabled {
            self.store.jobs.clone()
        } else {
            self.store
                .jobs
                .iter()
                .filter(|j| j.enabled)
                .cloned()
                .collect()
        }
    }

    /// Remove a job by its ID. Returns `true` if a job was removed.
    pub fn remove_job(&mut self, job_id: &str) -> bool {
        let before = self.store.jobs.len();
        self.store.jobs.retain(|j| j.id != job_id);
        let removed = self.store.jobs.len() < before;
        if removed {
            self.persist();
            info!("Cron: removed job {}", job_id);
        }
        removed
    }

    /// Enable or disable a job.
    pub fn enable_job(&mut self, job_id: &str, enabled: bool) -> Option<CronJob> {
        let job = self.store.jobs.iter_mut().find(|j| j.id == job_id)?;
        job.enabled = enabled;
        job.updated_at_ms = now_ms();
        let result = job.clone();
        self.persist();
        Some(result)
    }

    /// Get service status.
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({
            "enabled": self.running,
            "jobs": self.store.jobs.len(),
        })
    }

    /// Serialize the current store to disk.
    fn persist(&self) {
        if let Some(parent) = self.store_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        if let Ok(json) = serde_json::to_string_pretty(&self.store) {
            if let Err(e) = std::fs::write(&self.store_path, json) {
                warn!("Failed to persist cron store: {}", e);
            }
        }
    }
}
