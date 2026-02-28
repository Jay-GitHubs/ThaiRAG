pub mod tantivy_search;

use thairag_config::schema::TextSearchConfig;
use thairag_core::traits::TextSearch;
use thairag_core::types::TextSearchKind;

pub fn create_text_search(config: &TextSearchConfig) -> Box<dyn TextSearch> {
    match config.kind {
        TextSearchKind::Tantivy => Box::new(tantivy_search::TantivySearch::new(&config.index_path)),
    }
}
