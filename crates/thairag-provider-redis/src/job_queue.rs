use std::time::Duration;

use async_trait::async_trait;
use redis::AsyncCommands;
use thairag_core::traits::JobQueue;
use thairag_core::types::{Job, JobId, JobStatus, WorkspaceId};

use crate::RedisConnection;

/// Redis-backed job queue.
///
/// Storage layout:
/// - `job:{job_id}` — JSON-serialized `Job` with TTL for automatic cleanup
/// - `jobs:ws:{workspace_id}` — Redis SET of job ID strings for workspace lookup
pub struct RedisJobQueue {
    conn: RedisConnection,
    /// TTL in seconds for completed/failed/cancelled job keys (retention period).
    retention_secs: u64,
}

impl RedisJobQueue {
    pub fn new(conn: RedisConnection, retention_secs: u64) -> Self {
        Self {
            conn,
            retention_secs,
        }
    }

    fn job_key(job_id: &JobId) -> String {
        format!("job:{}", job_id.0)
    }

    fn workspace_set_key(workspace_id: &WorkspaceId) -> String {
        format!("jobs:ws:{}", workspace_id.0)
    }

    fn now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
    }

    /// Store a job in Redis, setting TTL only for terminal states.
    async fn save_job(&self, job: &Job) {
        let key = Self::job_key(&job.id);
        let json = match serde_json::to_string(job) {
            Ok(j) => j,
            Err(e) => {
                tracing::error!(error = %e, "Failed to serialize job");
                return;
            }
        };

        let mut conn = self.conn.manager();

        let is_terminal = matches!(
            job.status,
            JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled
        );

        if is_terminal {
            // Set with TTL for automatic cleanup
            let _: Result<(), _> = conn
                .set_ex::<_, _, ()>(&key, &json, self.retention_secs)
                .await;
        } else {
            // Active jobs: no TTL
            let _: Result<(), _> = conn.set::<_, _, ()>(&key, &json).await;
        }
    }
}

#[async_trait]
impl JobQueue for RedisJobQueue {
    async fn enqueue(&self, job: Job) -> JobId {
        let id = job.id;
        let ws_key = Self::workspace_set_key(&job.workspace_id);

        // Save the job
        self.save_job(&job).await;

        // Add job ID to workspace set
        let mut conn = self.conn.manager();
        let _: Result<(), _> = conn.sadd::<_, _, ()>(&ws_key, id.0.to_string()).await;

        id
    }

    async fn get(&self, job_id: &JobId) -> Option<Job> {
        let key = Self::job_key(job_id);
        let mut conn = self.conn.manager();
        let data: Option<String> = conn.get(&key).await.ok()?;
        let data = data?;
        serde_json::from_str(&data).ok()
    }

    async fn list_by_workspace(&self, workspace_id: &WorkspaceId) -> Vec<Job> {
        let ws_key = Self::workspace_set_key(workspace_id);
        let mut conn = self.conn.manager();

        // Get all job IDs in this workspace
        let job_ids: Vec<String> = conn.smembers(&ws_key).await.unwrap_or_default();

        if job_ids.is_empty() {
            return Vec::new();
        }

        // Pipeline GET for all job keys
        let mut pipe = redis::pipe();
        for jid in &job_ids {
            pipe.get(format!("job:{jid}"));
        }

        let results: Vec<Option<String>> = match pipe.query_async(&mut conn).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(error = %e, "Failed to pipeline GET jobs");
                return Vec::new();
            }
        };

        // Collect valid jobs; track expired IDs to clean up the workspace set
        let mut jobs = Vec::new();
        let mut expired_ids = Vec::new();

        for (idx, data) in results.into_iter().enumerate() {
            match data {
                Some(json) => {
                    if let Ok(job) = serde_json::from_str::<Job>(&json) {
                        jobs.push(job);
                    }
                }
                None => {
                    // Job key expired (TTL); remove from workspace set
                    expired_ids.push(&job_ids[idx]);
                }
            }
        }

        // Lazily remove expired job IDs from workspace set
        if !expired_ids.is_empty() {
            let mut pipe = redis::pipe();
            for eid in &expired_ids {
                pipe.srem(&ws_key, *eid);
            }
            let _: Result<(), _> = pipe.query_async(&mut conn).await;
        }

        // Sort most recent first
        jobs.sort_by_key(|j| std::cmp::Reverse(j.created_at));
        jobs
    }

    async fn update_status(&self, job_id: &JobId, status: JobStatus, error: Option<String>) {
        if let Some(mut job) = self.get(job_id).await {
            job.status = status;
            job.error = error;
            self.save_job(&job).await;
        }
    }

    async fn mark_running(&self, job_id: &JobId) {
        if let Some(mut job) = self.get(job_id).await {
            job.status = JobStatus::Running;
            job.started_at = Some(Self::now());
            self.save_job(&job).await;
        }
    }

    async fn mark_completed(&self, job_id: &JobId, items_processed: usize) {
        if let Some(mut job) = self.get(job_id).await {
            job.status = JobStatus::Completed;
            job.completed_at = Some(Self::now());
            job.items_processed = items_processed;
            self.save_job(&job).await;
        }
    }

    async fn mark_failed(&self, job_id: &JobId, error: String) {
        if let Some(mut job) = self.get(job_id).await {
            job.status = JobStatus::Failed;
            job.completed_at = Some(Self::now());
            job.error = Some(error);
            self.save_job(&job).await;
        }
    }

    async fn increment_progress(&self, job_id: &JobId) {
        if let Some(mut job) = self.get(job_id).await {
            job.items_processed += 1;
            self.save_job(&job).await;
        }
    }

    async fn cancel(&self, job_id: &JobId) -> bool {
        if let Some(mut job) = self.get(job_id).await
            && (job.status == JobStatus::Queued || job.status == JobStatus::Running)
        {
            job.status = JobStatus::Cancelled;
            job.completed_at = Some(Self::now());
            self.save_job(&job).await;
            return true;
        }
        false
    }

    async fn cleanup(&self, max_age: Duration) {
        // Redis TTL handles most cleanup automatically. This method scans for
        // any terminal jobs that might have been created before TTL was set,
        // or whose TTL is longer than max_age.
        let cutoff = Self::now() - max_age.as_secs() as i64;
        let mut conn = self.conn.manager();

        // SCAN for job:* keys
        let mut cursor: u64 = 0;
        loop {
            let (next_cursor, keys): (u64, Vec<String>) = match redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg("job:*")
                .arg("COUNT")
                .arg(100)
                .query_async(&mut conn)
                .await
            {
                Ok(r) => r,
                Err(_) => break,
            };

            for key in &keys {
                let data: Option<String> = match conn.get(key).await {
                    Ok(d) => d,
                    Err(_) => continue,
                };

                if let Some(json) = data
                    && let Ok(job) = serde_json::from_str::<Job>(&json)
                    && matches!(
                        job.status,
                        JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled
                    )
                {
                    let completed_time = job.completed_at.unwrap_or(job.created_at);
                    if completed_time <= cutoff {
                        let _: Result<(), _> = conn.del::<_, ()>(key).await;
                        // Also remove from workspace set
                        let ws_key = Self::workspace_set_key(&job.workspace_id);
                        let _: Result<(), _> =
                            conn.srem::<_, _, ()>(&ws_key, job.id.0.to_string()).await;
                    }
                }
            }

            cursor = next_cursor;
            if cursor == 0 {
                break;
            }
        }
    }

    async fn count(&self) -> usize {
        let mut conn = self.conn.manager();
        // SCAN for job:* keys and count them
        let mut total = 0usize;
        let mut cursor: u64 = 0;
        loop {
            let (next_cursor, keys): (u64, Vec<String>) = match redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg("job:*")
                .arg("COUNT")
                .arg(100)
                .query_async(&mut conn)
                .await
            {
                Ok(r) => r,
                Err(_) => break,
            };

            total += keys.len();
            cursor = next_cursor;
            if cursor == 0 {
                break;
            }
        }
        total
    }
}
