use std::time::Duration;

use async_trait::async_trait;
use dashmap::DashMap;
use thairag_core::traits::JobQueue;
use thairag_core::types::{Job, JobId, JobStatus, WorkspaceId};

/// In-memory job queue backed by `DashMap`.
pub struct InMemoryJobQueue {
    jobs: DashMap<JobId, Job>,
}

impl Default for InMemoryJobQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryJobQueue {
    pub fn new() -> Self {
        Self {
            jobs: DashMap::new(),
        }
    }

    fn now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
    }
}

#[async_trait]
impl JobQueue for InMemoryJobQueue {
    async fn enqueue(&self, job: Job) -> JobId {
        let id = job.id;
        self.jobs.insert(id, job);
        id
    }

    async fn get(&self, job_id: &JobId) -> Option<Job> {
        self.jobs.get(job_id).map(|r| r.clone())
    }

    async fn list_by_workspace(&self, workspace_id: &WorkspaceId) -> Vec<Job> {
        let mut jobs: Vec<Job> = self
            .jobs
            .iter()
            .filter(|r| r.workspace_id == *workspace_id)
            .map(|r| r.clone())
            .collect();
        jobs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        jobs
    }

    async fn update_status(&self, job_id: &JobId, status: JobStatus, error: Option<String>) {
        if let Some(mut job) = self.jobs.get_mut(job_id) {
            job.status = status;
            job.error = error;
        }
    }

    async fn mark_running(&self, job_id: &JobId) {
        if let Some(mut job) = self.jobs.get_mut(job_id) {
            job.status = JobStatus::Running;
            job.started_at = Some(Self::now());
        }
    }

    async fn mark_completed(&self, job_id: &JobId, items_processed: usize) {
        if let Some(mut job) = self.jobs.get_mut(job_id) {
            job.status = JobStatus::Completed;
            job.completed_at = Some(Self::now());
            job.items_processed = items_processed;
        }
    }

    async fn mark_failed(&self, job_id: &JobId, error: String) {
        if let Some(mut job) = self.jobs.get_mut(job_id) {
            job.status = JobStatus::Failed;
            job.completed_at = Some(Self::now());
            job.error = Some(error);
        }
    }

    async fn cancel(&self, job_id: &JobId) -> bool {
        if let Some(mut job) = self.jobs.get_mut(job_id)
            && (job.status == JobStatus::Queued || job.status == JobStatus::Running)
        {
            job.status = JobStatus::Cancelled;
            job.completed_at = Some(Self::now());
            return true;
        }
        false
    }

    async fn cleanup(&self, max_age: Duration) {
        let cutoff = Self::now() - max_age.as_secs() as i64;
        self.jobs.retain(|_, job| {
            // Keep active jobs always; remove finished jobs older than cutoff
            match job.status {
                JobStatus::Queued | JobStatus::Running => true,
                _ => job.completed_at.unwrap_or(job.created_at) > cutoff,
            }
        });
    }

    async fn count(&self) -> usize {
        self.jobs.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use thairag_core::types::JobKind;
    use uuid::Uuid;

    fn make_job(workspace_id: WorkspaceId, kind: JobKind) -> Job {
        Job {
            id: JobId(Uuid::new_v4()),
            kind,
            status: JobStatus::Queued,
            workspace_id,
            doc_id: None,
            description: "test job".into(),
            created_at: InMemoryJobQueue::now(),
            started_at: None,
            completed_at: None,
            error: None,
            items_processed: 0,
        }
    }

    #[tokio::test]
    async fn enqueue_and_get() {
        let q = InMemoryJobQueue::new();
        let ws = WorkspaceId(Uuid::new_v4());
        let job = make_job(ws, JobKind::DocumentIngestion);
        let id = job.id;

        q.enqueue(job).await;
        let got = q.get(&id).await.unwrap();
        assert_eq!(got.status, JobStatus::Queued);
        assert_eq!(got.workspace_id, ws);
    }

    #[tokio::test]
    async fn lifecycle_running_completed() {
        let q = InMemoryJobQueue::new();
        let ws = WorkspaceId(Uuid::new_v4());
        let job = make_job(ws, JobKind::DocumentReprocess);
        let id = job.id;
        q.enqueue(job).await;

        q.mark_running(&id).await;
        let j = q.get(&id).await.unwrap();
        assert_eq!(j.status, JobStatus::Running);
        assert!(j.started_at.is_some());

        q.mark_completed(&id, 42).await;
        let j = q.get(&id).await.unwrap();
        assert_eq!(j.status, JobStatus::Completed);
        assert_eq!(j.items_processed, 42);
        assert!(j.completed_at.is_some());
    }

    #[tokio::test]
    async fn cancel_queued_job() {
        let q = InMemoryJobQueue::new();
        let ws = WorkspaceId(Uuid::new_v4());
        let job = make_job(ws, JobKind::DocumentIngestion);
        let id = job.id;
        q.enqueue(job).await;

        assert!(q.cancel(&id).await);
        let j = q.get(&id).await.unwrap();
        assert_eq!(j.status, JobStatus::Cancelled);

        // Cannot cancel again
        assert!(!q.cancel(&id).await);
    }

    #[tokio::test]
    async fn list_filters_by_workspace() {
        let q = InMemoryJobQueue::new();
        let ws1 = WorkspaceId(Uuid::new_v4());
        let ws2 = WorkspaceId(Uuid::new_v4());

        q.enqueue(make_job(ws1, JobKind::DocumentIngestion)).await;
        q.enqueue(make_job(ws1, JobKind::DocumentReprocess)).await;
        q.enqueue(make_job(ws2, JobKind::BatchReprocess)).await;

        assert_eq!(q.list_by_workspace(&ws1).await.len(), 2);
        assert_eq!(q.list_by_workspace(&ws2).await.len(), 1);
    }

    #[tokio::test]
    async fn mark_failed() {
        let q = InMemoryJobQueue::new();
        let ws = WorkspaceId(Uuid::new_v4());
        let job = make_job(ws, JobKind::DocumentIngestion);
        let id = job.id;
        q.enqueue(job).await;

        q.mark_failed(&id, "oops".into()).await;
        let j = q.get(&id).await.unwrap();
        assert_eq!(j.status, JobStatus::Failed);
        assert_eq!(j.error.as_deref(), Some("oops"));
    }

    #[tokio::test]
    async fn cleanup_removes_old() {
        let q = InMemoryJobQueue::new();
        let ws = WorkspaceId(Uuid::new_v4());

        // Create a completed job with old timestamp
        let mut old_job = make_job(ws, JobKind::DocumentIngestion);
        old_job.status = JobStatus::Completed;
        old_job.completed_at = Some(0); // epoch = very old
        let old_id = old_job.id;
        q.enqueue(old_job).await;

        // Create a queued job (should be kept)
        let new_job = make_job(ws, JobKind::DocumentReprocess);
        let new_id = new_job.id;
        q.enqueue(new_job).await;

        q.cleanup(Duration::from_secs(3600)).await;

        assert!(q.get(&old_id).await.is_none());
        assert!(q.get(&new_id).await.is_some());
    }
}
