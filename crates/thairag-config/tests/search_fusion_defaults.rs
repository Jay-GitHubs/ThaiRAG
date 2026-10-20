//! The shipped fusion defaults must let a keyword-only (BM25) hit reach the
//! rerank window. RRF scores each hit `weight / (k + rank + 1)` and the
//! engine keeps the top `rerank_top_k` fused hits. The best keyword-only hit
//! therefore scores `text_weight / (k + 1)` and the last dense hit inside the
//! window `vector_weight / (k + rerank_top_k)`. With the former 0.6 / 0.4
//! split those were 0.0066 and 0.0092: keyword search could reorder the dense
//! top-k but never add a chunk dense search missed (a form code, a product
//! name, a section number). Balanced weights restore that ability.

use config::{Config, File};
use thairag_config::schema::SearchConfig;

#[test]
fn shipped_defaults_let_a_keyword_only_hit_enter_the_rerank_window() {
    let default =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/default.toml");
    let cfg = Config::builder()
        .add_source(File::from(default.as_path()).required(true))
        .build()
        .expect("config/default.toml must load");
    let search: SearchConfig = cfg.get("search").expect("[search] table must deserialize");

    let k = search.rrf_k as f32;
    let best_keyword_only = search.text_weight / (k + 1.0);
    let last_dense_in_window = search.vector_weight / (k + search.rerank_top_k as f32);
    assert!(
        best_keyword_only > last_dense_in_window,
        "a rank-1 keyword-only hit ({best_keyword_only:.5}) must outscore the last dense hit \
         inside the rerank window ({last_dense_in_window:.5}); weights {}/{}, k {}, window {}",
        search.vector_weight,
        search.text_weight,
        search.rrf_k,
        search.rerank_top_k
    );
}
