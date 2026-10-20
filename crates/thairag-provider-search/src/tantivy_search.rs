use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use async_trait::async_trait;
use tantivy::collector::TopDocs;
use tantivy::directory::MmapDirectory;
use tantivy::query::{BooleanQuery, Occur, Query, TermQuery};
use tantivy::schema::{
    Field, IndexRecordOption, STORED, STRING, Schema, TextFieldIndexing, TextOptions, Value,
};
use tantivy::{Index, IndexReader, IndexSettings, IndexWriter, ReloadPolicy, TantivyDocument};
use thairag_core::ThaiRagError;
use thairag_core::error::Result;
use thairag_core::traits::TextSearch;
use thairag_core::types::{ChunkId, DocId, DocumentChunk, SearchQuery, SearchResult, WorkspaceId};
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

/// Build schema + register Thai tokenizer (shared by create and open).
fn build_schema() -> (Schema, TantivyFields) {
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
    let fields = TantivyFields {
        chunk_id,
        doc_id,
        workspace_id,
        content,
        chunk_index,
    };
    (schema, fields)
}

fn register_thai_tokenizer(index: &Index) {
    let segmenter = DictionarySegmenter::new();
    let thai_tokenizer = ThaiTantivyTokenizer::new(segmenter.shared());
    index.tokenizers().register("thai", thai_tokenizer);
}

impl TantivySearch {
    pub fn new(index_path: &str) -> Self {
        let (schema, fields) = build_schema();
        let is_disk = !index_path.is_empty() && index_path != "test";

        // BM25 is DERIVED data — rebuildable from stored chunks via the
        // reindex endpoint. A hard kill mid-commit can leave meta.json
        // referencing segment files that were never fsynced; then open,
        // writer, or READER creation fails at startup. Panicking there
        // crash-loops the whole API (observed live: restart during ingestion
        // → missing .term file → Exited(101) loop). So: prove the index
        // usable end-to-end, and on any failure wipe the directory and start
        // empty instead of refusing to boot.
        let built = Self::try_build(index_path, is_disk, &schema, false).or_else(|e| {
            if is_disk {
                tracing::warn!(
                    index_path,
                    error = %e,
                    "Tantivy index unusable (crash-corrupted?) — recreating empty; \
                     run a workspace reindex to restore BM25"
                );
                Self::try_build(index_path, is_disk, &schema, true)
            } else {
                Err(e)
            }
        });
        let (index, writer, reader) =
            built.expect("Failed to initialize Tantivy index even after recreating it");

        Self {
            _index_path: index_path.to_string(),
            index,
            writer: Mutex::new(writer),
            reader,
            fields,
        }
    }

    /// Open-or-create the index and prove it usable by building the writer
    /// AND reader (both can fail on a crash-corrupted directory even when
    /// `meta.json` opens fine). `recreate` wipes the directory first — the
    /// recovery path.
    fn try_build(
        index_path: &str,
        is_disk: bool,
        schema: &Schema,
        recreate: bool,
    ) -> std::result::Result<(Index, IndexWriter, IndexReader), String> {
        let index = if is_disk {
            let dir = Path::new(index_path);
            if recreate {
                std::fs::remove_dir_all(dir).map_err(|e| format!("wipe index dir: {e}"))?;
            }
            if dir.join("meta.json").exists() {
                let idx = Index::open_in_dir(dir).map_err(|e| format!("open existing: {e}"))?;
                info!(index_path, "Opened existing Tantivy index from disk");
                idx
            } else {
                std::fs::create_dir_all(dir).map_err(|e| format!("create index dir: {e}"))?;
                let mmap_dir = MmapDirectory::open(dir).map_err(|e| format!("mmap dir: {e}"))?;
                let idx = Index::create(mmap_dir, schema.clone(), IndexSettings::default())
                    .map_err(|e| format!("create index: {e}"))?;
                info!(
                    index_path,
                    recreated = recreate,
                    "Created new Tantivy index on disk"
                );
                idx
            }
        } else {
            // Test mode: use RAM
            info!(index_path, "Created Tantivy index in RAM (test mode)");
            Index::create_in_ram(schema.clone())
        };

        register_thai_tokenizer(&index);

        // Clear stale lock file from previous crash/restart (single-writer assumption)
        if is_disk {
            let lock_file = Path::new(index_path).join(".tantivy-writer.lock");
            if lock_file.exists() {
                info!(index_path, "Removing stale Tantivy writer lock file");
                let _ = std::fs::remove_file(&lock_file);
            }
        }

        let writer = index
            .writer(50_000_000) // 50 MB heap
            .map_err(|e| format!("writer: {e}"))?;

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(|e: tantivy::TantivyError| format!("reader: {e}"))?;

        Ok((index, writer, reader))
    }

    /// Returns the number of documents currently in the index.
    pub fn doc_count(&self) -> u64 {
        let searcher = self.reader.searcher();
        searcher.num_docs()
    }

    /// Build the lexical query as an OR of the terms the content field's own
    /// analyzer produces for `text`.
    ///
    /// This deliberately bypasses Tantivy's `QueryParser`, which turns every
    /// whitespace-delimited word that tokenizes into more than one term into
    /// a slop-0 `PhraseQuery`. Thai writes no spaces between words, so a
    /// natural Thai question is ONE such word and could only match a chunk
    /// holding the identical token sequence: "วงเงินสูงสุด" (วงเงิน|สูงสุด)
    /// found nothing in a chunk that contained both words a few tokens
    /// apart, and a trailing question word ("…เท่าไหร่") made a match
    /// impossible. Running the query through the same registered analyzer
    /// the index uses (segmentation, stopwords, SARA AM normalization) and
    /// OR-ing the distinct terms gives BM25 its per-term scoring for Thai and
    /// English alike, and makes query punctuation (`?`, `:`, parentheses)
    /// inert instead of parser syntax.
    ///
    /// Returns `None` when the text yields no indexable term (empty, or
    /// stopwords/punctuation only): nothing can match, so the caller returns
    /// no results instead of running an empty query.
    fn build_text_query(&self, text: &str) -> Result<Option<Box<dyn Query>>> {
        let mut analyzer = self
            .index
            .tokenizer_for_field(self.fields.content)
            .map_err(|e| ThaiRagError::Internal(format!("Tantivy tokenizer lookup error: {e}")))?;
        let mut stream = analyzer.token_stream(text);
        let mut seen: HashSet<String> = HashSet::new();
        let mut clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();
        while stream.advance() {
            let token = &stream.token().text;
            if token.is_empty() || !seen.insert(token.clone()) {
                continue;
            }
            let term = tantivy::Term::from_field_text(self.fields.content, token);
            clauses.push((
                Occur::Should,
                Box::new(TermQuery::new(term, IndexRecordOption::WithFreqs)),
            ));
        }
        Ok(match clauses.len() {
            0 => None,
            1 => Some(clauses.swap_remove(0).1),
            _ => Some(Box::new(BooleanQuery::new(clauses))),
        })
    }
}

#[async_trait]
impl TextSearch for TantivySearch {
    async fn index(&self, chunks: &[DocumentChunk]) -> Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|e| ThaiRagError::Internal(format!("Tantivy writer lock poisoned: {e}")))?;

        for chunk in chunks {
            let mut doc = TantivyDocument::new();
            doc.add_text(self.fields.chunk_id, chunk.chunk_id.to_string());
            doc.add_text(self.fields.doc_id, chunk.doc_id.to_string());
            doc.add_text(self.fields.workspace_id, chunk.workspace_id.to_string());
            // Index the context prefix (document identity / enrichment
            // context) together with the content so lexical search can match
            // document-level terms (e.g. a program name) that the chunk body
            // never repeats.
            let indexed_text = match chunk
                .metadata
                .as_ref()
                .and_then(|m| m.context_prefix.as_ref())
            {
                Some(prefix) => format!("{prefix}\n{}", chunk.content),
                None => chunk.content.clone(),
            };
            doc.add_text(self.fields.content, &indexed_text);
            doc.add_u64(self.fields.chunk_index, chunk.chunk_index as u64);
            writer
                .add_document(doc)
                .map_err(|e| ThaiRagError::Internal(format!("Tantivy add_document error: {e}")))?;
        }

        writer
            .commit()
            .map_err(|e| ThaiRagError::Internal(format!("Tantivy commit error: {e}")))?;

        // Reload reader so newly committed docs are immediately searchable.
        self.reader
            .reload()
            .map_err(|e| ThaiRagError::Internal(format!("Tantivy reader reload error: {e}")))?;

        info!(count = chunks.len(), "Indexed chunks in Tantivy");
        Ok(())
    }

    async fn delete_by_doc(&self, doc_id: thairag_core::types::DocId) -> Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|e| ThaiRagError::Internal(format!("Tantivy writer lock poisoned: {e}")))?;

        let term = tantivy::Term::from_field_text(self.fields.doc_id, &doc_id.to_string());
        writer.delete_term(term);

        writer
            .commit()
            .map_err(|e| ThaiRagError::Internal(format!("Tantivy commit error: {e}")))?;

        self.reader
            .reload()
            .map_err(|e| ThaiRagError::Internal(format!("Tantivy reader reload error: {e}")))?;

        info!(%doc_id, "Deleted documents from Tantivy index");
        Ok(())
    }

    async fn delete_all(&self) -> Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|e| ThaiRagError::Internal(format!("Tantivy writer lock poisoned: {e}")))?;

        writer
            .delete_all_documents()
            .map_err(|e| ThaiRagError::Internal(format!("Tantivy delete_all error: {e}")))?;
        writer
            .commit()
            .map_err(|e| ThaiRagError::Internal(format!("Tantivy commit error: {e}")))?;

        self.reader
            .reload()
            .map_err(|e| ThaiRagError::Internal(format!("Tantivy reader reload error: {e}")))?;

        info!("Cleared all documents from Tantivy index");
        Ok(())
    }

    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>> {
        // No access: not unrestricted and no workspace permissions
        if !query.unrestricted && query.workspace_ids.is_empty() {
            return Ok(vec![]);
        }

        let searcher = self.reader.searcher();

        let Some(text_query) = self.build_text_query(&query.text)? else {
            // No indexable term (empty, or stopwords/punctuation only):
            // nothing can match.
            return Ok(vec![]);
        };

        // Build final query: text MUST, with optional workspace and document
        // filters ANDed on. doc_ids restricts to specific documents (agentic
        // doc-selection).
        let mut clauses: Vec<(Occur, Box<dyn tantivy::query::Query>)> =
            vec![(Occur::Must, text_query)];
        if !query.unrestricted && !query.workspace_ids.is_empty() {
            let ws_clauses: Vec<(Occur, Box<dyn tantivy::query::Query>)> = query
                .workspace_ids
                .iter()
                .map(|ws| {
                    let term =
                        tantivy::Term::from_field_text(self.fields.workspace_id, &ws.to_string());
                    (
                        Occur::Should,
                        Box::new(TermQuery::new(term, IndexRecordOption::Basic))
                            as Box<dyn tantivy::query::Query>,
                    )
                })
                .collect();
            clauses.push((Occur::Must, Box::new(BooleanQuery::new(ws_clauses))));
        }
        if !query.doc_ids.is_empty() {
            let doc_clauses: Vec<(Occur, Box<dyn tantivy::query::Query>)> = query
                .doc_ids
                .iter()
                .map(|d| {
                    let term = tantivy::Term::from_field_text(self.fields.doc_id, &d.to_string());
                    (
                        Occur::Should,
                        Box::new(TermQuery::new(term, IndexRecordOption::Basic))
                            as Box<dyn tantivy::query::Query>,
                    )
                })
                .collect();
            clauses.push((Occur::Must, Box::new(BooleanQuery::new(doc_clauses))));
        }
        let final_query: Box<dyn tantivy::query::Query> = if clauses.len() == 1 {
            clauses.pop().unwrap().1
        } else {
            Box::new(BooleanQuery::new(clauses))
        };

        let top_docs = searcher
            .search(&*final_query, &TopDocs::with_limit(query.top_k))
            .map_err(|e| ThaiRagError::Internal(format!("Tantivy search error: {e}")))?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher
                .doc(doc_address)
                .map_err(|e| ThaiRagError::Internal(format!("Tantivy doc retrieval error: {e}")))?;

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
                    metadata: None,
                },
                score,
                vector_score: None,
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
            metadata: None,
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
            unrestricted: true,
            query_images: Vec::new(),
            doc_ids: Vec::new(),
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
            unrestricted: true,
            query_images: Vec::new(),
            doc_ids: Vec::new(),
        };
        let results = search.search(&query).await.unwrap();
        assert!(
            !results.is_empty(),
            "English search should still work with Thai tokenizer"
        );
    }

    fn query(text: &str) -> SearchQuery {
        SearchQuery {
            text: text.to_string(),
            workspace_ids: vec![],
            top_k: 10,
            unrestricted: true,
            query_images: Vec::new(),
            doc_ids: Vec::new(),
        }
    }

    /// Regression: `QueryParser` turned an unspaced Thai query that segments
    /// into several words into a slop-0 phrase, so "วงเงินสูงสุด"
    /// (วงเงิน|สูงสุด) returned nothing against a chunk holding both words a
    /// few tokens apart. The query is now an OR of the analyzer's terms, and
    /// BM25 ranks the chunk matching both terms first.
    #[tokio::test]
    async fn thai_unspaced_query_matches_non_adjacent_terms() {
        let search = TantivySearch::new("test");
        let both_terms = make_chunk("ระยะเวลากู้สูงสุด 10 ปี โดยวงเงินขึ้นกับหลักประกัน");
        let one_term = make_chunk("วงเงินกู้สูงสุดไม่เกิน 5 ล้านบาทต่อราย");
        let both_id = both_terms.chunk_id.0;
        search.index(&[one_term, both_terms]).await.unwrap();

        let results = search.search(&query("วงเงินสูงสุด")).await.unwrap();
        assert!(
            !results.is_empty(),
            "OR-of-terms must match words that are not adjacent"
        );
        assert_eq!(
            results[0].chunk.chunk_id.0,
            both_id,
            "the chunk matching both terms must rank first: {:?}",
            results.iter().map(|r| &r.chunk.content).collect::<Vec<_>>()
        );
    }

    /// A question word after the content words ("…เท่าไหร่") used to make a
    /// phrase match impossible; as an OR it is just one more optional term.
    #[tokio::test]
    async fn thai_question_word_does_not_block_match() {
        let search = TantivySearch::new("test");
        search
            .index(&[make_chunk("ค่าธรรมเนียมการจัดการวงเงิน 1,000 บาท")])
            .await
            .unwrap();
        let results = search.search(&query("ค่าธรรมเนียมเท่าไหร่")).await.unwrap();
        assert!(
            !results.is_empty(),
            "a trailing question word must not block the match"
        );
    }

    /// Query punctuation is inert text, not parser syntax: no parse errors,
    /// no field prefixes, no boolean operators.
    #[tokio::test]
    async fn query_punctuation_is_inert() {
        let search = TantivySearch::new("test");
        search
            .index(&[make_chunk("hello world from Rust")])
            .await
            .unwrap();
        for q in ["rust?", "rust: (hello)", "\"rust", "hello AND NOT world"] {
            let results = search.search(&query(q)).await.unwrap();
            assert!(!results.is_empty(), "query {q:?} must match");
        }
    }

    /// Multi-word queries have OR semantics: a term absent from the index
    /// does not veto the match.
    #[tokio::test]
    async fn multiword_query_is_or_of_terms() {
        let search = TantivySearch::new("test");
        search
            .index(&[make_chunk("hello world from Rust")])
            .await
            .unwrap();
        let results = search.search(&query("rust tokio")).await.unwrap();
        assert!(
            !results.is_empty(),
            "an unknown term must not veto the match"
        );
    }

    /// A query that yields no indexable term (empty, stopwords or punctuation
    /// only) returns nothing instead of erroring or matching everything.
    #[tokio::test]
    async fn stopword_only_query_returns_empty() {
        let search = TantivySearch::new("test");
        search
            .index(&[make_chunk("ห้องสมุดเปิดให้บริการทุกวัน")])
            .await
            .unwrap();
        for q in ["ของ ที่ และ", "", "?!..."] {
            let results = search.search(&query(q)).await.unwrap();
            assert!(results.is_empty(), "query {q:?} must match nothing");
        }
    }

    /// A hard kill mid-commit leaves meta.json referencing segment files that
    /// were never written. Startup must recover to an empty index (BM25 is
    /// derived data) instead of panicking into a crash-loop — the exact live
    /// failure: missing `.term` file → reader creation failed → Exited(101).
    #[tokio::test]
    async fn crash_corrupted_index_recovers_instead_of_panicking() {
        let dir = std::env::temp_dir().join(format!("tantivy-recovery-{}", Uuid::new_v4()));
        let path = dir.to_str().unwrap().to_string();

        // Seed a valid on-disk index with one committed document.
        {
            let search = TantivySearch::new(&path);
            search.index(&[make_chunk("seed content")]).await.unwrap();
            assert_eq!(search.doc_count(), 1);
        }

        // Corruption mode 1 (the live one): delete a segment file that
        // meta.json still references.
        let seg = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .find(|e| e.path().extension().is_some_and(|x| x == "term"))
            .expect("committed index must have a .term segment file");
        std::fs::remove_file(seg.path()).unwrap();
        let search = TantivySearch::new(&path); // must not panic
        assert_eq!(search.doc_count(), 0, "recovered index starts empty");
        drop(search);

        // Corruption mode 2: unparseable meta.json.
        std::fs::write(dir.join("meta.json"), "{ not json").unwrap();
        let search = TantivySearch::new(&path); // must not panic
        assert_eq!(search.doc_count(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
