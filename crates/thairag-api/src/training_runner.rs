use std::sync::Arc;

use dashmap::DashMap;
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;
use tokio::sync::oneshot;

use crate::app_state::AppState;
use crate::store::KmStoreTrait;

/// Manages running fine-tuning subprocess jobs.
pub struct TrainingRunner {
    running_jobs: Arc<DashMap<String, RunningJob>>,
    logs: Arc<DashMap<String, Vec<String>>>,
}

struct RunningJob {
    _child_pid: u32,
    cancel_tx: Option<oneshot::Sender<()>>,
}

impl Default for TrainingRunner {
    fn default() -> Self {
        Self {
            running_jobs: Arc::new(DashMap::new()),
            logs: Arc::new(DashMap::new()),
        }
    }
}

impl TrainingRunner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a fine-tuning training subprocess for the given job.
    pub async fn start_training(&self, state: &AppState, job_id: &str) -> Result<(), String> {
        // Get job from store, verify it's pending
        let job = state
            .km_store
            .get_finetune_job(job_id)
            .map_err(|e| format!("Job not found: {e}"))?;

        if job.status != "pending" {
            return Err(format!("Job is '{}', expected 'pending'", job.status));
        }

        // Get dataset and pairs
        let pairs = state.km_store.list_training_pairs(&job.dataset_id);
        if pairs.is_empty() {
            return Err("Dataset has no training pairs".into());
        }

        // Export dataset to temp Alpaca JSONL
        let output_dir = format!(
            "{}/job-{}",
            state.config.embedding_finetune.finetune_output_dir, job_id
        );
        std::fs::create_dir_all(&output_dir)
            .map_err(|e| format!("Failed to create output dir: {e}"))?;

        let data_path = format!("{}/dataset.jsonl", output_dir);
        let mut jsonl_lines = Vec::with_capacity(pairs.len());
        for pair in &pairs {
            let entry = serde_json::json!({
                "instruction": pair.query,
                "input": "",
                "output": pair.positive_doc,
            });
            jsonl_lines.push(serde_json::to_string(&entry).unwrap_or_default());
        }
        std::fs::write(&data_path, jsonl_lines.join("\n"))
            .map_err(|e| format!("Failed to write dataset: {e}"))?;

        // Parse training config from job
        let config: serde_json::Value = job
            .config
            .as_deref()
            .and_then(|c| serde_json::from_str(c).ok())
            .unwrap_or_else(|| serde_json::json!({}));

        // Build command args
        let python_path = &state.config.embedding_finetune.python_path;
        let script_path = &state.config.embedding_finetune.finetune_script_path;

        let mut cmd = Command::new(python_path);
        cmd.arg(script_path)
            .arg("--base-model")
            .arg(&job.base_model)
            .arg("--data-path")
            .arg(&data_path)
            .arg("--output-dir")
            .arg(&output_dir);

        // Map config fields to CLI args
        if let Some(v) = config.get("epochs").and_then(|v| v.as_u64()) {
            cmd.arg("--epochs").arg(v.to_string());
        }
        if let Some(v) = config.get("learning_rate").and_then(|v| v.as_f64()) {
            cmd.arg("--lr").arg(v.to_string());
        }
        if let Some(v) = config.get("lora_rank").and_then(|v| v.as_u64()) {
            cmd.arg("--lora-rank").arg(v.to_string());
        }
        if let Some(v) = config.get("lora_alpha").and_then(|v| v.as_u64()) {
            cmd.arg("--lora-alpha").arg(v.to_string());
        }
        if let Some(v) = config.get("batch_size").and_then(|v| v.as_u64()) {
            cmd.arg("--batch-size").arg(v.to_string());
        }
        if let Some(v) = config.get("warmup_ratio").and_then(|v| v.as_f64()) {
            cmd.arg("--warmup-ratio").arg(v.to_string());
        }
        if let Some(v) = config.get("max_seq_length").and_then(|v| v.as_u64()) {
            cmd.arg("--max-seq-length").arg(v.to_string());
        }
        if let Some(v) = config.get("quantization").and_then(|v| v.as_str()) {
            cmd.arg("--quantization").arg(v);
        }
        if let Some(v) = config.get("model_source").and_then(|v| v.as_str()) {
            cmd.arg("--model-source").arg(v);
        }

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn training process: {e}"))?;
        let pid = child.id().unwrap_or(0);

        // Update job status to running
        state
            .km_store
            .update_finetune_job_status(job_id, "running", None)
            .map_err(|e| format!("Failed to update status: {e}"))?;

        // Setup cancellation
        let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
        self.running_jobs.insert(
            job_id.to_string(),
            RunningJob {
                _child_pid: pid,
                cancel_tx: Some(cancel_tx),
            },
        );
        self.logs.insert(job_id.to_string(), Vec::new());

        // Spawn reader task
        let job_id_owned = job_id.to_string();
        let store = Arc::clone(&state.km_store);
        let running_jobs = Arc::clone(&self.running_jobs);
        let logs = Arc::clone(&self.logs);

        tokio::spawn(async move {
            let stdout = child.stdout.take();
            let stderr = child.stderr.take();

            // Read stdout in a task
            let logs_clone = Arc::clone(&logs);
            let store_clone = Arc::clone(&store);
            let job_id_clone = job_id_owned.clone();

            let reader_handle = if let Some(stdout) = stdout {
                let reader = tokio::io::BufReader::new(stdout);
                let mut lines = reader.lines();
                Some(tokio::spawn(async move {
                    let mut last_metrics = String::new();
                    let mut last_output_path: Option<String> = None;
                    while let Ok(Some(line)) = lines.next_line().await {
                        // Store log line
                        if let Some(mut log_entry) = logs_clone.get_mut(&job_id_clone) {
                            log_entry.push(line.clone());
                        }

                        // Parse JSON progress
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line)
                            && let Some(msg_type) = json.get("type").and_then(|t| t.as_str())
                        {
                            match msg_type {
                                "progress" => {
                                    last_metrics = line.clone();
                                    let _ = store_clone.update_finetune_job_status(
                                        &job_id_clone,
                                        "running",
                                        Some(&line),
                                    );
                                }
                                "completed" => {
                                    last_metrics = line.clone();
                                    last_output_path = json
                                        .get("output_path")
                                        .and_then(|p| p.as_str())
                                        .map(|s| s.to_string());
                                }
                                _ => {}
                            }
                        }
                    }
                    (last_metrics, last_output_path)
                }))
            } else {
                None
            };

            // Also capture stderr
            if let Some(stderr) = stderr {
                let logs_clone2 = Arc::clone(&logs);
                let job_id_clone2 = job_id_owned.clone();
                tokio::spawn(async move {
                    let reader = tokio::io::BufReader::new(stderr);
                    let mut lines = reader.lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        if let Some(mut log_entry) = logs_clone2.get_mut(&job_id_clone2) {
                            log_entry.push(format!("[stderr] {line}"));
                        }
                    }
                });
            }

            // Wait for either process exit or cancellation
            tokio::select! {
                status = child.wait() => {
                    let (last_metrics, last_output_path) = if let Some(handle) = reader_handle {
                        handle.await.unwrap_or_default()
                    } else {
                        (String::new(), None)
                    };

                    match status {
                        Ok(exit) if exit.success() => {
                            let _ = store.update_finetune_job_full(
                                &job_id_owned,
                                "completed",
                                if last_metrics.is_empty() { None } else { Some(&last_metrics) },
                                last_output_path.as_deref(),
                            );
                        }
                        Ok(exit) => {
                            let err_msg = serde_json::json!({
                                "type": "error",
                                "message": format!("Process exited with code {}", exit.code().unwrap_or(-1)),
                            }).to_string();
                            let _ = store.update_finetune_job_full(
                                &job_id_owned,
                                "failed",
                                Some(&err_msg),
                                None,
                            );
                        }
                        Err(e) => {
                            let err_msg = serde_json::json!({
                                "type": "error",
                                "message": format!("Process error: {e}"),
                            }).to_string();
                            let _ = store.update_finetune_job_full(
                                &job_id_owned,
                                "failed",
                                Some(&err_msg),
                                None,
                            );
                        }
                    }
                }
                _ = cancel_rx => {
                    // Send SIGTERM
                    let _ = child.kill().await;
                    let _ = store.update_finetune_job_status(&job_id_owned, "cancelled", None);
                }
            }

            running_jobs.remove(&job_id_owned);
        });

        Ok(())
    }

    /// Cancel a running training job.
    pub fn cancel_training(&self, job_id: &str) -> Result<(), String> {
        if let Some((_, mut job)) = self.running_jobs.remove(job_id) {
            if let Some(tx) = job.cancel_tx.take() {
                let _ = tx.send(());
            }
            Ok(())
        } else {
            Err(format!("Job {job_id} is not currently running"))
        }
    }

    /// Get collected log lines for a job.
    pub fn get_logs(&self, job_id: &str) -> Vec<String> {
        self.logs
            .get(job_id)
            .map(|entry| entry.clone())
            .unwrap_or_default()
    }

    /// Check if a job is currently running.
    pub fn is_running(&self, job_id: &str) -> bool {
        self.running_jobs.contains_key(job_id)
    }

    /// On startup, mark any jobs stuck in "running" as "failed" (server restarted).
    pub fn recover_interrupted_jobs(store: &dyn KmStoreTrait) {
        let jobs = store.list_finetune_jobs();
        for job in jobs {
            if job.status == "running" {
                let msg = serde_json::json!({
                    "type": "error",
                    "message": "Server restarted while job was running",
                })
                .to_string();
                let _ = store.update_finetune_job_full(&job.id, "failed", Some(&msg), None);
                tracing::warn!(job_id = %job.id, "Marked interrupted finetune job as failed");
            }
        }
    }
}
