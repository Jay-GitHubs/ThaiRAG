//! V2 API endpoints.
//!
//! - `POST /v2/chat/completions` — enhanced chat with structured metadata
//! - `POST /v2/search` — dedicated search endpoint (no LLM generation)
//! - `GET  /v2/models` — models list with version info
//! - `GET  /api/version` — API version info

pub mod v2_chat;
pub mod v2_models;
pub mod v2_search;
pub mod version_info;
