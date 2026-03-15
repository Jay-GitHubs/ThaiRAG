use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use thairag_core::error::Result;
use thairag_core::traits::VectorStore;
use thairag_core::types::{ChunkId, DocId, DocumentChunk, SearchQuery, SearchResult, WorkspaceId};
use thairag_core::ThaiRagError;
use tracing::{info, instrument};

pub struct PgvectorStore {
    pool: PgPool,
    collection: String,
    table_ready: AtomicBool,
}

impl PgvectorStore {
    pub fn new(db_url: &str, collection: &str) -> Self {
        let pool = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Enable pgvector extension
                let pool = PgPoolOptions::new()
                    .max_connections(5)
                    .connect(db_url)
                    .await
                    .expect("Failed to connect to PostgreSQL for pgvector");

                sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
                    .execute(&pool)
                    .await
                    .expect("Failed to enable pgvector extension");

                pool
            })
        });

        info!(collection, "Initialized pgvector store");

        Self {
            pool,
            collection: collection.to_string(),
            table_ready: AtomicBool::new(false),
        }
    }

    async fn ensure_table(&self, dim: usize) -> Result<()> {
        if self.table_ready.load(Ordering::Relaxed) {
            return Ok(());
        }

        let sql = format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                id TEXT PRIMARY KEY, \
                doc_id TEXT NOT NULL, \
                workspace_id TEXT NOT NULL, \
                content TEXT NOT NULL, \
                chunk_index INTEGER NOT NULL, \
                embedding vector({}))",
            self.collection, dim
        );

        sqlx::query(&sql)
            .execute(&self.pool)
            .await
            .map_err(|e| ThaiRagError::VectorStore(format!("Failed to create table: {e}")))?;

        self.table_ready.store(true, Ordering::Relaxed);
        Ok(())
    }
}

#[async_trait]
impl VectorStore for PgvectorStore {
    #[instrument(skip(self, chunks), fields(collection = %self.collection, chunk_count = chunks.len()))]
    async fn upsert(&self, chunks: &[DocumentChunk]) -> Result<()> {
        if chunks.is_empty() {
            return Ok(());
        }

        // Determine dimension from the first chunk that has an embedding
        if let Some(dim) = chunks
            .iter()
            .find_map(|c| c.embedding.as_ref().map(|e| e.len()))
        {
            self.ensure_table(dim).await?;
        }

        for chunk in chunks {
            let embedding = match chunk.embedding.as_ref() {
                Some(e) => e,
                None => continue,
            };

            // Format embedding as pgvector literal: [1.0,2.0,3.0]
            let emb_str = format!(
                "[{}]",
                embedding
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            );

            let sql = format!(
                "INSERT INTO {} (id, doc_id, workspace_id, content, chunk_index, embedding) \
                 VALUES ($1, $2, $3, $4, $5, $6::vector) \
                 ON CONFLICT(id) DO UPDATE SET \
                 doc_id = EXCLUDED.doc_id, \
                 workspace_id = EXCLUDED.workspace_id, \
                 content = EXCLUDED.content, \
                 chunk_index = EXCLUDED.chunk_index, \
                 embedding = EXCLUDED.embedding",
                self.collection
            );

            sqlx::query(&sql)
                .bind(chunk.chunk_id.0.to_string())
                .bind(chunk.doc_id.0.to_string())
                .bind(chunk.workspace_id.0.to_string())
                .bind(&chunk.content)
                .bind(chunk.chunk_index as i32)
                .bind(&emb_str)
                .execute(&self.pool)
                .await
                .map_err(|e| ThaiRagError::VectorStore(format!("pgvector upsert failed: {e}")))?;
        }

        Ok(())
    }

    #[instrument(skip(self, embedding), fields(collection = %self.collection, top_k = query.top_k))]
    async fn search(&self, embedding: &[f32], query: &SearchQuery) -> Result<Vec<SearchResult>> {
        if !query.unrestricted && query.workspace_ids.is_empty() {
            return Ok(vec![]);
        }

        let emb_str = format!(
            "[{}]",
            embedding
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );

        let (sql, workspace_strings) = if query.unrestricted {
            let sql = format!(
                "SELECT id, doc_id, workspace_id, content, chunk_index, \
                 1 - (embedding <=> $1::vector) AS score \
                 FROM {} \
                 ORDER BY embedding <=> $1::vector \
                 LIMIT $2",
                self.collection
            );
            (sql, vec![])
        } else {
            let ws: Vec<String> = query
                .workspace_ids
                .iter()
                .map(|id| id.0.to_string())
                .collect();
            let sql = format!(
                "SELECT id, doc_id, workspace_id, content, chunk_index, \
                 1 - (embedding <=> $1::vector) AS score \
                 FROM {} \
                 WHERE workspace_id = ANY($3) \
                 ORDER BY embedding <=> $1::vector \
                 LIMIT $2",
                self.collection
            );
            (sql, ws)
        };

        if query.unrestricted {
            let rows: Vec<(String, String, String, String, i32, f64)> = sqlx::query_as(&sql)
                .bind(&emb_str)
                .bind(query.top_k as i64)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| {
                    ThaiRagError::VectorStore(format!("pgvector search failed: {e}"))
                })?;

            Ok(rows
                .into_iter()
                .filter_map(|(id, doc_id, ws_id, content, chunk_index, score)| {
                    Some(SearchResult {
                        chunk: DocumentChunk {
                            chunk_id: ChunkId(id.parse().ok()?),
                            doc_id: DocId(doc_id.parse().ok()?),
                            workspace_id: WorkspaceId(ws_id.parse().ok()?),
                            content,
                            chunk_index: chunk_index as usize,
                            embedding: None,
                            metadata: None,
                        },
                        score: score as f32,
                    })
                })
                .collect())
        } else {
            let rows: Vec<(String, String, String, String, i32, f64)> = sqlx::query_as(&sql)
                .bind(&emb_str)
                .bind(query.top_k as i64)
                .bind(&workspace_strings)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| {
                    ThaiRagError::VectorStore(format!("pgvector search failed: {e}"))
                })?;

            Ok(rows
                .into_iter()
                .filter_map(|(id, doc_id, ws_id, content, chunk_index, score)| {
                    Some(SearchResult {
                        chunk: DocumentChunk {
                            chunk_id: ChunkId(id.parse().ok()?),
                            doc_id: DocId(doc_id.parse().ok()?),
                            workspace_id: WorkspaceId(ws_id.parse().ok()?),
                            content,
                            chunk_index: chunk_index as usize,
                            embedding: None,
                            metadata: None,
                        },
                        score: score as f32,
                    })
                })
                .collect())
        }
    }

    #[instrument(skip(self), fields(collection = %self.collection, doc_id = %doc_id))]
    async fn delete_by_doc(&self, doc_id: DocId) -> Result<()> {
        let sql = format!("DELETE FROM {} WHERE doc_id = $1", self.collection);

        sqlx::query(&sql)
            .bind(doc_id.0.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| ThaiRagError::VectorStore(format!("pgvector delete failed: {e}")))?;

        Ok(())
    }
}
