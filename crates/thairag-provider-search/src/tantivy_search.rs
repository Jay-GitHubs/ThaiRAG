use std::sync::Mutex;

use async_trait::async_trait;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Occur, QueryParser, TermQuery};
use tantivy::schema::{
    Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value, STORED, STRING,
};
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument};
use thairag_core::error::Result;
use thairag_core::traits::TextSearch;
use thairag_core::types::{ChunkId, DocId, DocumentChunk, SearchQuery, SearchResult, WorkspaceId};
use thairag_core::ThaiRagError;
use thairag_thai::{DictionarySegmenter, ThaiTantivyTokenizer};
use tracing::info;
use uuid::Uuid;

pub struct TantivySearch {
    _index_path: String,
    index: Index,
    writer: Mutex<IndexWriter>,
    reader: IndexReader,
    fields: TantivyFields,
}

struct TantivyFields {
    chunk_id: Field,
    doc_id: Field,
    workspace_id: Field,
    content: Field,
    chunk_index: Field,
}

impl TantivySearch {
    pub fn new(index_path: &str) -> Self {
        info!(index_path, "Creating Tantivy index (RamDirectory; index_path reserved for future disk persistence)");

        let mut schema_builder = Schema::builder();

        let chunk_id = schema_builder.add_text_field("chunk_id", STRING | STORED);
        let doc_id = schema_builder.add_text_field("doc_id", STRING | STORED);
        let workspace_id = schema_builder.add_text_field("workspace_id", STRING | STORED);

        let text_options = TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("thai")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored();
        let content = schema_builder.add_text_field("content", text_options);

        let chunk_index = schema_builder.add_u64_field("chunk_index", STORED);

        let schema = schema_builder.build();
        let index = Index::create_in_ram(schema);

        // Register Thai tokenizer (nlpo3 dictionary-based segmentation).
        let segmenter = DictionarySegmenter::new();
        let thai_tokenizer = ThaiTantivyTokenizer::new(segmenter.shared());
        index.tokenizers().register("thai", thai_tokenizer);

        let writer = index
            .writer(50_000_000) // 50 MB heap
            .expect("Failed to create Tantivy IndexWriter");

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .expect("Failed to create Tantivy IndexReader");

        Self {
            _index_path: index_path.to_string(),
            index,
            writer: Mutex::new(writer),
            reader,
            fields: TantivyFields {
                chunk_id,
                doc_id,
                workspace_id,
                content,
                chunk_index,
            },
        }
    }
}

#[async_trait]
impl TextSearch for TantivySearch {
    async fn index(&self, chunks: &[DocumentChunk]) -> Result<()> {
        let mut writer = self.writer.lock().map_err(|e| {
            ThaiRagError::Internal(format!("Tantivy writer lock poisoned: {e}"))
        })?;

        for chunk in chunks {
            let mut doc = TantivyDocument::new();
            doc.add_text(self.fields.chunk_id, chunk.chunk_id.to_string());
            doc.add_text(self.fields.doc_id, chunk.doc_id.to_string());
            doc.add_text(self.fields.workspace_id, chunk.workspace_id.to_string());
            doc.add_text(self.fields.content, &chunk.content);
            doc.add_u64(self.fields.chunk_index, chunk.chunk_index as u64);
            writer.add_document(doc).map_err(|e| {
                ThaiRagError::Internal(format!("Tantivy add_document error: {e}"))
            })?;
        }

        writer.commit().map_err(|e| {
            ThaiRagError::Internal(format!("Tantivy commit error: {e}"))
        })?;

        // Reload reader so newly committed docs are immediately searchable.
        self.reader.reload().map_err(|e| {
            ThaiRagError::Internal(format!("Tantivy reader reload error: {e}"))
        })?;

        info!(count = chunks.len(), "Indexed chunks in Tantivy");
        Ok(())
    }

    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>> {
        let searcher = self.reader.searcher();

        let query_parser = QueryParser::for_index(&self.index, vec![self.fields.content]);

        let text_query = query_parser.parse_query(&query.text).map_err(|e| {
            ThaiRagError::Internal(format!("Tantivy query parse error: {e}"))
        })?;

        // Build final query: text + workspace filter
        let final_query = if query.workspace_ids.is_empty() {
            text_query
        } else {
            // OR of workspace_id terms
            let ws_clauses: Vec<(Occur, Box<dyn tantivy::query::Query>)> = query
                .workspace_ids
                .iter()
                .map(|ws| {
                    let term = tantivy::Term::from_field_text(
                        self.fields.workspace_id,
                        &ws.to_string(),
                    );
                    (
                        Occur::Should,
                        Box::new(TermQuery::new(term, IndexRecordOption::Basic))
                            as Box<dyn tantivy::query::Query>,
                    )
                })
                .collect();
            let ws_query = BooleanQuery::new(ws_clauses);

            Box::new(BooleanQuery::new(vec![
                (Occur::Must, text_query),
                (Occur::Must, Box::new(ws_query)),
            ]))
        };

        let top_docs = searcher
            .search(&*final_query, &TopDocs::with_limit(query.top_k))
            .map_err(|e| ThaiRagError::Internal(format!("Tantivy search error: {e}")))?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher.doc(doc_address).map_err(|e| {
                ThaiRagError::Internal(format!("Tantivy doc retrieval error: {e}"))
            })?;

            let chunk_id_str: &str = doc
                .get_first(self.fields.chunk_id)
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let doc_id_str: &str = doc
                .get_first(self.fields.doc_id)
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let ws_id_str: &str = doc
                .get_first(self.fields.workspace_id)
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let content: String = doc
                .get_first(self.fields.content)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let chunk_index: usize = doc
                .get_first(self.fields.chunk_index)
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;

            results.push(SearchResult {
                chunk: DocumentChunk {
                    chunk_id: ChunkId(chunk_id_str.parse::<Uuid>().unwrap_or_default()),
                    doc_id: DocId(doc_id_str.parse::<Uuid>().unwrap_or_default()),
                    workspace_id: WorkspaceId(ws_id_str.parse::<Uuid>().unwrap_or_default()),
                    content,
                    chunk_index,
                    embedding: None,
                },
                score,
            });
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use thairag_core::traits::TextSearch;

    fn make_chunk(content: &str) -> DocumentChunk {
        DocumentChunk {
            chunk_id: ChunkId(Uuid::new_v4()),
            doc_id: DocId(Uuid::new_v4()),
            workspace_id: WorkspaceId(Uuid::nil()),
            content: content.to_string(),
            chunk_index: 0,
            embedding: None,
        }
    }

    #[tokio::test]
    async fn thai_search_finds_segmented_word() {
        let search = TantivySearch::new("test");

        // Index a Thai document.
        let chunk = make_chunk("ห้องสมุดเปิดให้บริการทุกวัน");
        search.index(&[chunk]).await.unwrap();

        // Search for a Thai word that appears in the segmented tokens.
        let query = SearchQuery {
            text: "ห้องสมุด".to_string(),
            workspace_ids: vec![],
            top_k: 10,
        };
        let results = search.search(&query).await.unwrap();
        assert!(
            !results.is_empty(),
            "Thai search for 'ห้องสมุด' should find the indexed document"
        );
    }

    #[tokio::test]
    async fn english_search_still_works() {
        let search = TantivySearch::new("test");

        let chunk = make_chunk("hello world from Rust");
        search.index(&[chunk]).await.unwrap();

        let query = SearchQuery {
            text: "hello".to_string(),
            workspace_ids: vec![],
            top_k: 10,
        };
        let results = search.search(&query).await.unwrap();
        assert!(
            !results.is_empty(),
            "English search should still work with Thai tokenizer"
        );
    }
}
