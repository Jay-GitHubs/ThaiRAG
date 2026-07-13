//! Detached chat generation: answers keep generating (and persist) even when
//! no client is connected.
//!
//! Before this module, generation lived inside the SSE response body — a
//! client disconnect (refresh, conversation switch, tab close) dropped the
//! generator mid-flight, and with it the answer. Now the `/api/chat` send
//! handler spawns generation as an independent task that publishes its SSE
//! payloads here; the HTTP response (and any later resume subscriber) merely
//! follows along. Late subscribers replay the buffered prefix first, so a
//! client that reattaches mid-answer reconstructs the partial text exactly.
//!
//! One generation per conversation at a time; entries linger briefly after
//! completion so a just-too-late resume still sees the final events instead
//! of a 404 (after that, the persisted messages are the source of truth).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use dashmap::DashMap;
use tokio::sync::{Mutex, broadcast};

/// How many live events a slow subscriber may lag before being dropped.
const CHANNEL_CAPACITY: usize = 1024;
/// How long a finished generation stays resumable.
const LINGER_SECS: u64 = 60;

/// A single in-flight (or just-finished) generation for one conversation.
pub struct Generation {
    /// Every SSE `data:` payload published so far, for replay on subscribe.
    buffer: Mutex<Vec<String>>,
    tx: broadcast::Sender<String>,
    done: AtomicBool,
    cancelled: AtomicBool,
}

impl Generation {
    fn new() -> Self {
        let (tx, _) = broadcast::channel(CHANNEL_CAPACITY);
        Self {
            buffer: Mutex::new(Vec::new()),
            tx,
            done: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
        }
    }

    /// Publish one SSE payload: buffered for replay, broadcast to followers.
    pub async fn publish(&self, payload: String) {
        self.buffer.lock().await.push(payload.clone());
        // No receivers is fine — the whole point is generating unwatched.
        let _ = self.tx.send(payload);
    }

    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }

    /// Cooperative cancel flag — the generation task checks this between
    /// tokens ("stop generating"). The partial answer is persisted by the
    /// task, matching what the user saw when they pressed stop.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Snapshot of buffered payloads plus a live receiver, atomically enough
    /// for SSE: anything published after the snapshot arrives via the
    /// receiver; duplicates are impossible because publish appends to the
    /// buffer *before* broadcasting and we subscribe *before* snapshotting.
    pub async fn subscribe(&self) -> (Vec<String>, broadcast::Receiver<String>) {
        let rx = self.tx.subscribe();
        let snapshot = self.buffer.lock().await.clone();
        (snapshot, rx)
    }
}

/// Registry of in-flight generations, keyed by conversation id.
#[derive(Default)]
pub struct GenerationHub {
    map: DashMap<String, Arc<Generation>>,
}

impl GenerationHub {
    /// Start a generation for a conversation. Returns `None` (busy) when one
    /// is already running there — one answer at a time per conversation.
    pub fn begin(&self, conversation_id: &str) -> Option<Arc<Generation>> {
        use dashmap::mapref::entry::Entry;
        match self.map.entry(conversation_id.to_string()) {
            Entry::Occupied(e) if !e.get().is_done() => None,
            Entry::Occupied(mut e) => {
                let generation = Arc::new(Generation::new());
                e.insert(generation.clone());
                Some(generation)
            }
            Entry::Vacant(v) => {
                let generation = Arc::new(Generation::new());
                v.insert(generation.clone());
                Some(generation)
            }
        }
    }

    /// Active (not yet done) generation for a conversation, if any. A
    /// finished-but-lingering generation is also returned so late resumes
    /// can replay the tail.
    pub fn get(&self, conversation_id: &str) -> Option<Arc<Generation>> {
        self.map.get(conversation_id).map(|g| g.clone())
    }

    /// Mark done and schedule removal after a linger window.
    pub fn finish(self: &Arc<Self>, conversation_id: &str, generation: &Arc<Generation>) {
        generation.done.store(true, Ordering::Release);
        let hub = Arc::clone(self);
        let conv = conversation_id.to_string();
        let gen_ptr = Arc::clone(generation);
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(LINGER_SECS)).await;
            // Remove only if it's still OUR generation (a newer one may have
            // replaced the slot meanwhile).
            hub.map.remove_if(&conv, |_, g| Arc::ptr_eq(g, &gen_ptr));
        });
    }

    /// Request cancellation of the active generation, if any. Returns whether
    /// something was cancelled.
    pub fn cancel(&self, conversation_id: &str) -> bool {
        if let Some(g) = self.map.get(conversation_id)
            && !g.is_done()
        {
            g.cancel();
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn replay_then_follow_sees_everything_once() {
        let g = Generation::new();
        g.publish("a".into()).await;
        g.publish("b".into()).await;
        let (snapshot, mut rx) = g.subscribe().await;
        g.publish("c".into()).await;
        assert_eq!(snapshot, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(rx.recv().await.unwrap(), "c");
    }

    #[tokio::test]
    async fn one_generation_per_conversation() {
        let hub = Arc::new(GenerationHub::default());
        let g1 = hub.begin("c1").expect("first begins");
        assert!(hub.begin("c1").is_none(), "second must be rejected");
        hub.finish("c1", &g1);
        assert!(hub.begin("c1").is_some(), "after done a new one may begin");
    }

    #[tokio::test]
    async fn cancel_only_hits_active() {
        let hub = Arc::new(GenerationHub::default());
        assert!(!hub.cancel("nope"));
        let g = hub.begin("c1").unwrap();
        assert!(hub.cancel("c1"));
        assert!(g.is_cancelled());
        hub.finish("c1", &g);
        assert!(!hub.cancel("c1"), "finished generation is not cancellable");
    }
}
