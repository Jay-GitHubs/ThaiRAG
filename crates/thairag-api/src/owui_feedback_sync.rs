//! OWUI → ThaiRAG feedback sync bridge.
//!
//! OWUI's native thumbs up/down write only to OWUI's own database, never to
//! ThaiRAG. This periodic task pulls those ratings back in. It polls OWUI's
//! admin-gated export endpoint, resolves each rating to the originating ThaiRAG
//! request via the `thairag_response_id` we stamped into the OpenAI `usage`
//! object (which OWUI persists verbatim into its feedback snapshot), and feeds
//! it through the same [`apply_feedback`] path the HTTP handler uses so both
//! sources drive the auto-tuning learners identically.
//!
//! Strict no-op when `owui_feedback_sync.enabled` is false: the task is never
//! spawned and no network call is made.

use std::time::Duration;

use serde_json::Value;
use tracing::{debug, info, warn};

use crate::app_state::AppState;
use crate::routes::feedback::{FeedbackRequest, apply_feedback};

/// km_store setting holding the high-water mark (max OWUI `updated_at` seen).
/// Rows at or below this timestamp are skipped so a rating is applied once.
const WATERMARK_KEY: &str = "owui_feedback_sync:watermark";

/// Spawn the periodic OWUI feedback sync loop if enabled in config.
pub fn spawn_owui_feedback_sync(state: AppState) {
    let cfg = state.config.owui_feedback_sync.clone();
    if !cfg.enabled {
        return;
    }
    if cfg.base_url.is_empty() || cfg.admin_api_key.is_empty() {
        warn!("OWUI feedback sync enabled but base_url or admin_api_key is empty; not starting");
        return;
    }

    info!(
        base_url = %cfg.base_url,
        interval_secs = cfg.interval_secs,
        "Starting OWUI feedback sync"
    );

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(cfg.interval_secs.max(1)));
        loop {
            interval.tick().await;
            match run_sync_pass(&state).await {
                Ok(n) if n > 0 => info!(applied = n, "OWUI feedback sync applied new ratings"),
                Ok(_) => debug!("OWUI feedback sync: no new ratings"),
                Err(e) => warn!(error = %e, "OWUI feedback sync pass failed"),
            }
        }
    });
}

/// Run one sync pass: fetch the export, apply newly-rated rows, advance the
/// watermark. Returns the number of ratings applied.
async fn run_sync_pass(state: &AppState) -> Result<usize, String> {
    let cfg = &state.config.owui_feedback_sync;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(cfg.request_timeout_secs.max(1)))
        .build()
        .map_err(|e| format!("http client build: {e}"))?;

    let url = format!(
        "{}/api/v1/evaluations/feedbacks/all/export",
        cfg.base_url.trim_end_matches('/')
    );

    let resp = client
        .get(&url)
        .bearer_auth(&cfg.admin_api_key)
        .send()
        .await
        .map_err(|e| format!("request to {url}: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("export endpoint returned HTTP {}", resp.status()));
    }

    let feedbacks: Vec<Value> = resp
        .json()
        .await
        .map_err(|e| format!("parse export json: {e}"))?;

    let watermark = state
        .km_store
        .get_setting(WATERMARK_KEY)
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);

    let mut max_seen = watermark;
    let mut applied = 0usize;

    for fb in &feedbacks {
        let updated_at = fb.get("updated_at").and_then(Value::as_i64).unwrap_or(0);
        if updated_at <= watermark {
            continue;
        }
        if updated_at > max_seen {
            max_seen = updated_at;
        }

        let Some(req) = build_feedback_request(fb) else {
            // No resolvable response_id or a neutral/absent rating — skip.
            continue;
        };

        let user_id = fb
            .get("user_id")
            .and_then(Value::as_str)
            .map(|u| format!("owui:{u}"))
            .unwrap_or_else(|| "owui:unknown".to_string());

        apply_feedback(state, req, user_id);
        applied += 1;
    }

    if max_seen > watermark {
        state
            .km_store
            .set_setting(WATERMARK_KEY, &max_seen.to_string());
    }

    Ok(applied)
}

/// Build a minimal [`FeedbackRequest`] from one OWUI feedback row, or `None` if
/// the rating is neutral/absent or no ThaiRAG response id can be resolved. The
/// remaining context (query/workspace/lineage) is backfilled server-side by
/// `enrich_feedback_context` inside `apply_feedback`.
fn build_feedback_request(fb: &Value) -> Option<FeedbackRequest> {
    let data = fb.get("data")?;
    let thumbs_up = match data.get("rating") {
        Some(Value::Number(n)) => match n.as_i64().unwrap_or(0) {
            r if r > 0 => true,
            r if r < 0 => false,
            _ => return None,
        },
        Some(Value::String(s)) => match s.trim() {
            "1" => true,
            "-1" => false,
            _ => return None,
        },
        _ => return None,
    };

    let response_id = resolve_response_id(fb)?;

    let comment = data
        .get("comment")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    Some(FeedbackRequest {
        response_id,
        thumbs_up,
        comment,
        query: None,
        answer: None,
        workspace_id: None,
        doc_ids: Vec::new(),
        chunk_scores: Vec::new(),
        chunk_ids: Vec::new(),
    })
}

/// Resolve the ThaiRAG `chatcmpl-…` id from an OWUI feedback row by walking
/// `snapshot.chat.(chat.)history.messages[meta.message_id].usage.thairag_response_id`.
///
/// OWUI's admin export serializes the whole ChatModel, whose actual chat content
/// lives in a nested `chat` column — so the real v0.9.6 shape is
/// `snapshot.chat.chat.history…`. We try that first and fall back to a flat
/// `snapshot.chat.history…` so a raw chat object (or a future shape change) still
/// resolves. The fallback is why we tolerate both rather than hard-coding one.
fn resolve_response_id(fb: &Value) -> Option<String> {
    let message_id = fb.get("meta")?.get("message_id")?.as_str()?;
    let chat = fb.get("snapshot")?.get("chat")?;
    let history = chat
        .get("chat")
        .and_then(|c| c.get("history"))
        .or_else(|| chat.get("history"))?;
    let id = history
        .get("messages")?
        .get(message_id)?
        .get("usage")?
        .get("thairag_response_id")?
        .as_str()?;
    Some(id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Mirrors the REAL OWUI v0.9.6 admin-export shape: the exported `snapshot.
    /// chat` is the serialized ChatModel, whose content lives in a nested `chat`
    /// column — so messages live at `snapshot.chat.chat.history.messages`.
    fn row(message_id: &str, rating: Value, response_id: Option<&str>) -> Value {
        let usage = match response_id {
            Some(id) => json!({ "thairag_response_id": id, "total_tokens": 42 }),
            None => json!({ "total_tokens": 42 }),
        };
        json!({
            "user_id": "u1",
            "updated_at": 1000,
            "data": { "rating": rating, "comment": "nice" },
            "meta": { "message_id": message_id },
            "snapshot": {
                "chat": {
                    "chat": { "history": { "messages": { message_id: { "usage": usage } } } }
                }
            }
        })
    }

    #[test]
    fn resolves_response_id_from_snapshot() {
        let fb = row("m1", json!(1), Some("chatcmpl-abc"));
        assert_eq!(resolve_response_id(&fb).as_deref(), Some("chatcmpl-abc"));
    }

    #[test]
    fn resolves_response_id_from_flat_snapshot_fallback() {
        // A raw/flat chat object (no ChatModel wrapper) must still resolve.
        let fb = json!({
            "meta": { "message_id": "m1" },
            "snapshot": {
                "chat": {
                    "history": { "messages": { "m1": {
                        "usage": { "thairag_response_id": "chatcmpl-flat" }
                    } } }
                }
            }
        });
        assert_eq!(resolve_response_id(&fb).as_deref(), Some("chatcmpl-flat"));
    }

    #[test]
    fn missing_response_id_yields_none() {
        let fb = row("m1", json!(1), None);
        assert!(resolve_response_id(&fb).is_none());
        assert!(build_feedback_request(&fb).is_none());
    }

    #[test]
    fn maps_positive_rating_to_thumbs_up() {
        let fb = row("m1", json!(1), Some("chatcmpl-abc"));
        let req = build_feedback_request(&fb).unwrap();
        assert!(req.thumbs_up);
        assert_eq!(req.response_id, "chatcmpl-abc");
        assert_eq!(req.comment.as_deref(), Some("nice"));
    }

    #[test]
    fn maps_negative_rating_to_thumbs_down() {
        let fb = row("m1", json!(-1), Some("chatcmpl-xyz"));
        let req = build_feedback_request(&fb).unwrap();
        assert!(!req.thumbs_up);
    }

    #[test]
    fn string_ratings_are_accepted() {
        assert!(
            build_feedback_request(&row("m1", json!("1"), Some("r")))
                .unwrap()
                .thumbs_up
        );
        assert!(
            !build_feedback_request(&row("m1", json!("-1"), Some("r")))
                .unwrap()
                .thumbs_up
        );
    }

    #[test]
    fn neutral_or_absent_rating_is_skipped() {
        assert!(build_feedback_request(&row("m1", json!(0), Some("r"))).is_none());
        assert!(build_feedback_request(&row("m1", json!("0"), Some("r"))).is_none());
        assert!(build_feedback_request(&row("m1", Value::Null, Some("r"))).is_none());
    }
}
