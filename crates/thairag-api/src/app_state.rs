use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use thairag_agent::active_learning::ActiveLearning;
use thairag_agent::colbert_reranker::ColbertReranker;
use thairag_agent::context_compactor::ContextCompactor;
use thairag_agent::context_curator::ContextCurator;
use thairag_agent::contextual_compression::ContextualCompression;
use thairag_agent::conversation_memory::ConversationMemory;
use thairag_agent::corrective_rag::CorrectiveRag;
use thairag_agent::graph_rag::GraphRag;
use thairag_agent::language_adapter::LanguageAdapter;
use thairag_agent::live_retrieval::LiveRetrieval;
use thairag_agent::map_reduce::MapReduceRag;
use thairag_agent::multimodal_rag::MultimodalRag;
use thairag_agent::personal_memory::PersonalMemoryManager;
use thairag_agent::quality_guard::QualityGuard;
use thairag_agent::query_analyzer::QueryAnalyzer;
use thairag_agent::query_rewriter::QueryRewriter;
use thairag_agent::ragas_eval::RagasEvaluator;
use thairag_agent::raptor::Raptor;
use thairag_agent::response_generator::ResponseGenerator;
use thairag_agent::self_rag::SelfRag;
use thairag_agent::speculative_rag::SpeculativeRag;
use thairag_agent::structured_extraction::StructuredExtractor;
use thairag_agent::tool_router::ToolRouter;
use thairag_agent::{ChatPipeline, PipelineOrchestrator, QueryOrchestrator, RagEngine};
use thairag_auth::JwtService;
use thairag_config::AppConfig;
use thairag_config::schema::{ChatPipelineConfig, DocumentConfig, ProvidersConfig, SearchConfig};
use thairag_core::PromptRegistry;
use thairag_core::traits::{
    EmbeddingModel, ImageEmbeddingModel, LlmProvider, Reranker, TextSearch, VectorStore,
};
use thairag_document::DocumentPipeline;
use thairag_search::HybridSearchEngine;

use thairag_provider_embedding::{
    create_embedding_provider_with_options, create_image_embedding_provider,
};
use thairag_provider_llm::{create_llm_provider, create_llm_provider_with_options};
use thairag_provider_reranker::create_reranker;
use thairag_provider_search::create_text_search;
use thairag_provider_vectordb::{create_personal_memory_store, create_vector_store};

use crate::embedding_cache::{InMemoryEmbeddingCache, NoopEmbeddingCache};
use crate::login_tracker::LoginTracker;
use crate::metrics::MetricsState;
use crate::oidc::OidcStateCache;
use crate::session::InMemorySessionStore;
use crate::store::{KmStoreTrait, create_km_store};
use crate::vault::Vault;

/// Resolve a `LlmConfig` through the vault profile system.
///
/// If the config has a `profile_id`, we look up the corresponding
/// `LlmProfileRow` in the store, decrypt the API key from the vault,
/// and return a fully resolved `LlmConfig`. Otherwise we return a clone.
fn resolve_profile(
    config: &thairag_config::schema::LlmConfig,
    store: Option<&dyn KmStoreTrait>,
    vault: &Vault,
) -> thairag_config::schema::LlmConfig {
    if let Some(ref pid) = config.profile_id
        && let Some(store) = store
        && let Some(profile) = store.get_llm_profile(pid)
    {
        let api_key = profile
            .vault_key_id
            .as_deref()
            .and_then(|kid| store.get_vault_key(kid))
            .map(|vk| vault.decrypt(&vk.encrypted_key).unwrap_or_default())
            .unwrap_or_default();

        let kind =
            crate::routes::settings::parse_llm_kind(&profile.kind).unwrap_or(config.kind.clone());

        return thairag_config::schema::LlmConfig {
            kind,
            model: profile.model,
            base_url: profile.base_url,
            api_key,
            max_tokens: profile.max_tokens.or(config.max_tokens),
            profile_id: Some(pid.clone()),
            ollama_num_ctx_max: config.ollama_num_ctx_max,
            temperature: config.temperature,
            thinking_enabled: config.thinking_enabled,
            supports_vision: config.supports_vision,
        };
    }
    config.clone()
}

/// Dynamic provider state that can be hot-swapped by super admin.
#[derive(Clone)]
pub struct ProviderBundle {
    pub providers_config: ProvidersConfig,
    pub chat_pipeline_config: ChatPipelineConfig,
    pub orchestrator: Arc<QueryOrchestrator>,
    pub chat_pipeline: Option<Arc<ChatPipeline>>,
    pub document_pipeline: Arc<DocumentPipeline>,
    pub search_engine: Arc<HybridSearchEngine>,
    pub embedding: Arc<dyn EmbeddingModel>,
    /// CLIP image-embedding model, present only when `image_embedding.enabled`.
    /// Used at ingest to compute per-image vectors; `None` = visual search off.
    pub image_embedding: Option<Arc<dyn ImageEmbeddingModel>>,
    pub reranker: Arc<dyn Reranker>,
    pub context_compactor: Option<Arc<ContextCompactor>>,
    pub personal_memory_manager: Option<Arc<PersonalMemoryManager>>,
    /// The primary chat LLM, already constructed (profile-resolved, retrying).
    /// For one-off LLM calls outside the pipeline (e.g. conversation
    /// summarize) — reuse this instead of re-creating a provider from config.
    pub chat_llm: Arc<dyn LlmProvider>,
}

/// Fluent builder for [`ProviderBundle`].
///
/// Required inputs are passed to [`ProviderBundleBuilder::new`]; everything
/// optional (km store, vault, embedding cache, plugin engine) is set via
/// `with_*` methods. Call `.build()` to construct the bundle. Replaces the
/// previous 9-argument `build_full_with_cache(...)` call sites that grew
/// unwieldy as new optional plumbing was added.
pub struct ProviderBundleBuilder<'a> {
    providers: &'a ProvidersConfig,
    search: &'a SearchConfig,
    doc: &'a DocumentConfig,
    chat: &'a ChatPipelineConfig,
    prompts: Arc<thairag_core::PromptRegistry>,
    km_store: Option<Arc<dyn crate::store::KmStoreTrait>>,
    vault: Option<&'a Vault>,
    embedding_cache: Option<Arc<dyn thairag_core::traits::EmbeddingCache>>,
    plugin_engine: Option<Arc<dyn thairag_core::traits::SearchPluginEngine>>,
    guardrail_metrics: Option<Arc<dyn thairag_core::traits::GuardrailMetricsRecorder>>,
}

impl<'a> ProviderBundleBuilder<'a> {
    pub fn new(
        providers: &'a ProvidersConfig,
        search: &'a SearchConfig,
        doc: &'a DocumentConfig,
        chat: &'a ChatPipelineConfig,
        prompts: Arc<thairag_core::PromptRegistry>,
    ) -> Self {
        Self {
            providers,
            search,
            doc,
            chat,
            prompts,
            km_store: None,
            vault: None,
            embedding_cache: None,
            plugin_engine: None,
            guardrail_metrics: None,
        }
    }

    pub fn with_km_store(mut self, km_store: Arc<dyn crate::store::KmStoreTrait>) -> Self {
        self.km_store = Some(km_store);
        self
    }

    pub fn with_vault(mut self, vault: &'a Vault) -> Self {
        self.vault = Some(vault);
        self
    }

    pub fn with_embedding_cache(
        mut self,
        cache: Arc<dyn thairag_core::traits::EmbeddingCache>,
    ) -> Self {
        self.embedding_cache = Some(cache);
        self
    }

    pub fn with_plugin_engine(
        mut self,
        engine: Arc<dyn thairag_core::traits::SearchPluginEngine>,
    ) -> Self {
        self.plugin_engine = Some(engine);
        self
    }

    pub fn with_guardrail_metrics(
        mut self,
        recorder: Arc<dyn thairag_core::traits::GuardrailMetricsRecorder>,
    ) -> Self {
        self.guardrail_metrics = Some(recorder);
        self
    }

    pub fn build(self) -> ProviderBundle {
        ProviderBundle::build_internal(
            self.providers,
            self.search,
            self.doc,
            self.chat,
            self.prompts,
            self.km_store,
            self.vault,
            self.embedding_cache,
            self.plugin_engine,
            self.guardrail_metrics,
        )
    }
}

impl ProviderBundle {
    #[allow(clippy::too_many_arguments)]
    fn build_internal(
        providers: &ProvidersConfig,
        search: &SearchConfig,
        doc: &DocumentConfig,
        chat: &ChatPipelineConfig,
        prompts: Arc<thairag_core::PromptRegistry>,
        km_store: Option<Arc<dyn crate::store::KmStoreTrait>>,
        vault: Option<&Vault>,
        embedding_cache: Option<Arc<dyn thairag_core::traits::EmbeddingCache>>,
        plugin_engine: Option<Arc<dyn thairag_core::traits::SearchPluginEngine>>,
        guardrail_metrics: Option<Arc<dyn thairag_core::traits::GuardrailMetricsRecorder>>,
    ) -> Self {
        let ollama_ka = &chat.ollama_keep_alive;
        let ka_opt = if ollama_ka.is_empty() {
            None
        } else {
            Some(ollama_ka.as_str())
        };

        // Resolve the main LLM config through the vault profile system
        let resolved_llm_cfg = if let Some(v) = vault {
            resolve_profile(&providers.llm, km_store.as_ref().map(|s| s.as_ref()), v)
        } else {
            providers.llm.clone()
        };
        let llm: Arc<dyn LlmProvider> = Arc::from(create_llm_provider_with_options(
            &resolved_llm_cfg,
            120,
            ka_opt,
        ));
        let raw_embedding: Arc<dyn EmbeddingModel> = Arc::from(
            create_embedding_provider_with_options(&providers.embedding, ka_opt),
        );
        let embedding: Arc<dyn EmbeddingModel> = if let Some(cache) = embedding_cache {
            Arc::new(crate::cached_embedding::CachedEmbeddingModel::new(
                raw_embedding,
                cache,
            ))
        } else {
            raw_embedding
        };
        let vector_store: Arc<dyn VectorStore> =
            Arc::from(create_vector_store(&providers.vector_store));
        let text_search: Arc<dyn TextSearch> =
            Arc::from(create_text_search(&providers.text_search));
        let reranker: Arc<dyn Reranker> = Arc::from(create_reranker(&providers.reranker));

        let mut engine = HybridSearchEngine::new(
            Arc::clone(&embedding),
            vector_store,
            text_search,
            Arc::clone(&reranker),
            search.clone(),
        );

        // Optional CLIP visual search. When enabled, build the image-embedding
        // model and a sibling `{collection}_clip` vector store (same isolation
        // routing as the text store) and attach both to the engine. The image
        // model is also kept on the bundle so ingest can precompute vectors.
        let image_embedding: Option<Arc<dyn ImageEmbeddingModel>> = match &providers.image_embedding
        {
            Some(img_cfg) if img_cfg.enabled => {
                let img_provider: Arc<dyn ImageEmbeddingModel> =
                    Arc::from(create_image_embedding_provider(img_cfg));
                let mut clip_vs_cfg = providers.vector_store.clone();
                clip_vs_cfg.collection = format!("{}_clip", clip_vs_cfg.collection);
                let clip_store: Arc<dyn VectorStore> = Arc::from(create_vector_store(&clip_vs_cfg));
                engine =
                    engine.with_image_search(Arc::clone(&img_provider), clip_store, img_cfg.weight);
                Some(img_provider)
            }
            _ => None,
        };

        let search_engine = Arc::new(engine);

        let rag_engine = Arc::new(RagEngine::new_with_prompts(
            Arc::clone(&llm),
            Arc::clone(&search_engine),
            Arc::clone(&prompts),
        ));
        let orchestrator = Arc::new(QueryOrchestrator::new_with_prompts(
            Arc::clone(&llm),
            rag_engine,
            Arc::clone(&prompts),
        ));

        // Resolve per-agent LLM providers with fallback chain:
        //   agent-specific config → shared preprocessing LLM → main chat LLM
        // Bulk-lane wrapped (exactly once per handle — see Throttled docs):
        // ingestion fan-out must queue instead of saturating the gateway's
        // slot pool and starving interactive chat traffic.
        let shared_preprocessing_llm: Arc<dyn LlmProvider> =
            Arc::new(thairag_core::backpressure::Throttled(
                if let Some(ref cfg) = doc.ai_preprocessing.llm {
                    let resolved = if let Some(v) = vault {
                        resolve_profile(cfg, km_store.as_ref().map(|s| s.as_ref()), v)
                    } else {
                        cfg.clone()
                    };
                    Arc::from(create_llm_provider(&resolved))
                } else {
                    Arc::clone(&llm)
                },
            ));

        let store_ref = km_store.as_ref().map(|s| s.as_ref());
        let resolve_agent_llm =
            |agent_cfg: &Option<thairag_config::schema::LlmConfig>| -> Arc<dyn LlmProvider> {
                if let Some(cfg) = agent_cfg {
                    let resolved = if let Some(v) = vault {
                        resolve_profile(cfg, store_ref, v)
                    } else {
                        cfg.clone()
                    };
                    Arc::new(thairag_core::backpressure::Throttled(Arc::from(
                        create_llm_provider(&resolved),
                    )))
                } else {
                    // shared_preprocessing_llm is already bulk-wrapped.
                    Arc::clone(&shared_preprocessing_llm)
                }
            };

        let analyzer_llm = resolve_agent_llm(&doc.ai_preprocessing.analyzer_llm);
        let converter_llm = resolve_agent_llm(&doc.ai_preprocessing.converter_llm);
        let quality_llm = resolve_agent_llm(&doc.ai_preprocessing.quality_llm);
        let chunker_llm = resolve_agent_llm(&doc.ai_preprocessing.chunker_llm);
        let enricher_llm = if doc.ai_preprocessing.enricher_enabled {
            Some(resolve_agent_llm(&doc.ai_preprocessing.enricher_llm))
        } else {
            None
        };
        let orchestrator_llm = if doc.ai_preprocessing.orchestrator_enabled {
            Some(resolve_agent_llm(&doc.ai_preprocessing.orchestrator_llm))
        } else {
            None
        };

        let document_pipeline = {
            let pipeline = DocumentPipeline::new_with_per_agent_ai_and_prompts(
                doc.max_chunk_size,
                doc.chunk_overlap,
                analyzer_llm,
                converter_llm,
                quality_llm,
                chunker_llm,
                enricher_llm,
                orchestrator_llm,
                &doc.ai_preprocessing,
                Arc::clone(&prompts),
            )
            .with_table_extraction(doc.table_extraction_enabled)
            .with_pdf_vision_fallback(
                doc.pdf_vision_fallback_enabled,
                doc.pdf_min_chars_per_page,
                doc.pdf_max_vision_pages,
            )
            .with_smart_pdf_options(
                doc.pdf_image_dpi,
                doc.max_image_edge,
                doc.pdf_page_as_image_threshold,
                doc.pdf_min_image_size,
                doc.pdf_max_images_per_page,
                doc.pdf_high_quality,
                doc.pdf_image_enhance,
                doc.pdf_vision_concurrency,
                doc.pdf_table_rescue_enabled,
                doc.pdf_table_rescue_max_pages,
            );

            // Always attach a vision model so the smart-PDF path can OCR pages
            // that genuinely need it — image/scanned pages, or pages whose text
            // layer is corrupted by a broken ToUnicode CMap (the `เรืĻอง`
            // garbling). Vision OCR is ENFORCED for PDFs: it is not gated by the
            // opt-in `image_description_enabled` toggle (which now only gates
            // DOCX/XLSX/HTML embedded-image + direct-image description). The
            // routing is adaptive, so vision only actually fires per-page where
            // needed — clean text and deterministic tables never call it.
            //
            // Prefer the dedicated `providers.doc_vision_llm` so operators can
            // keep a fast text-only chat model for `llm` while routing OCR to a
            // heavier vision-capable model (Ollama `llava`/`qwen2.5-vl`, Claude
            // 3+, GPT-4o). Falls back to the primary LLM when unset — which only
            // works if the primary model is itself vision-capable; pipeline.rs
            // fails loud with a structured EmptyExtraction reason if it isn't.
            // Bulk-lane wrapped: page OCR / table rescue / image description
            // are pure ingestion fan-out.
            let vision_llm: Arc<dyn LlmProvider> = Arc::new(thairag_core::backpressure::Throttled(
                if let Some(ref cfg) = providers.doc_vision_llm {
                    let resolved = if let Some(v) = vault {
                        resolve_profile(cfg, store_ref, v)
                    } else {
                        cfg.clone()
                    };
                    if !cfg.model.is_empty() {
                        tracing::info!(
                            kind = ?resolved.kind,
                            model = %resolved.model,
                            "Using dedicated vision LLM for document pipeline"
                        );
                    }
                    Arc::from(create_llm_provider(&resolved))
                } else {
                    tracing::info!(
                        "No `providers.doc_vision_llm` configured — falling back to primary LLM for \
                         PDF OCR / image description. This only works when the primary model is \
                         itself vision-capable."
                    );
                    Arc::clone(&llm)
                },
            ));
            let pipeline =
                pipeline.with_image_description(vision_llm, doc.image_description_enabled);

            // Deterministic OCR tier (PaddleOCR sidecar) — opt-in via
            // `document.ocr_sidecar_url`. When set, OCR-needing PDF pages prefer
            // it over the vision LLM. Default-off: empty URL ⇒ pipeline unchanged.
            let pipeline =
                match thairag_document::ocr::SidecarOcrProvider::new(&doc.ocr_sidecar_url) {
                    Some(provider) => {
                        tracing::info!(
                            url = %doc.ocr_sidecar_url,
                            "Deterministic OCR tier enabled (PaddleOCR sidecar)"
                        );
                        pipeline.with_ocr_provider(Arc::new(provider))
                    }
                    None => pipeline,
                };

            Arc::new(pipeline)
        };

        // ── Chat Pipeline (multi-agent) ──
        let chat_pipeline = if chat.enabled {
            let chat_timeout = chat.request_timeout_secs;
            let chat_shared_llm: Arc<dyn LlmProvider> = if let Some(ref cfg) = chat.llm {
                let resolved = if let Some(v) = vault {
                    resolve_profile(cfg, store_ref, v)
                } else {
                    cfg.clone()
                };
                Arc::from(create_llm_provider_with_options(
                    &resolved,
                    chat_timeout,
                    ka_opt,
                ))
            } else {
                Arc::clone(&llm)
            };

            let resolve_chat_agent_llm = |agent_name: &str,
                                          agent_cfg: &Option<thairag_config::schema::LlmConfig>|
             -> Arc<dyn LlmProvider> {
                if let Some(cfg) = agent_cfg {
                    let resolved = if let Some(v) = vault {
                        resolve_profile(cfg, store_ref, v)
                    } else {
                        cfg.clone()
                    };
                    tracing::info!(
                        agent = agent_name,
                        kind = ?resolved.kind,
                        model = %resolved.model,
                        "Chat agent: using per-agent LLM"
                    );
                    Arc::from(create_llm_provider_with_options(
                        &resolved,
                        chat_timeout,
                        ka_opt,
                    ))
                } else {
                    tracing::info!(
                        agent = agent_name,
                        "Chat agent: falling back to shared/main LLM"
                    );
                    Arc::clone(&chat_shared_llm)
                }
            };

            let max_tok = chat.agent_max_tokens;

            let qa = if chat.query_analyzer_enabled {
                Some(QueryAnalyzer::new_with_prompts(
                    resolve_chat_agent_llm("query_analyzer", &chat.query_analyzer_llm),
                    max_tok.min(256),
                    Arc::clone(&prompts),
                ))
            } else {
                None
            };

            let qr = if chat.query_rewriter_enabled {
                Some(QueryRewriter::new_with_prompts(
                    resolve_chat_agent_llm("query_rewriter", &chat.query_rewriter_llm),
                    max_tok.min(512),
                    Arc::clone(&prompts),
                ))
            } else {
                None
            };

            let cc = if chat.context_curator_enabled {
                Some(ContextCurator::new_with_prompts(
                    resolve_chat_agent_llm("context_curator", &chat.context_curator_llm),
                    chat.max_context_tokens,
                    max_tok.min(256),
                    Arc::clone(&prompts),
                ))
            } else {
                None
            };

            // Dedicated chat-answer vision LLM. Unlike per-agent LLMs, this
            // stays `None` when unconfigured so the ResponseGenerator falls
            // back to its own answer LLM (which only sees images if it is
            // itself vision-capable) rather than the shared chat LLM.
            let chat_vision_llm: Option<Arc<dyn LlmProvider>> =
                chat.chat_vision_llm.as_ref().map(|cfg| {
                    let resolved = if let Some(v) = vault {
                        resolve_profile(cfg, store_ref, v)
                    } else {
                        cfg.clone()
                    };
                    tracing::info!(
                        kind = ?resolved.kind,
                        model = %resolved.model,
                        "Chat: using dedicated vision LLM for answer-time image input"
                    );
                    Arc::from(create_llm_provider_with_options(
                        &resolved,
                        chat_timeout,
                        ka_opt,
                    )) as Arc<dyn LlmProvider>
                });

            let rg = ResponseGenerator::new_with_prompts(
                resolve_chat_agent_llm("response_generator", &chat.response_generator_llm),
                Arc::clone(&prompts),
            )
            .with_vision_llm(chat_vision_llm);

            let se = if chat.structured_extraction_enabled {
                Some(StructuredExtractor::new_with_prompts(
                    resolve_chat_agent_llm(
                        "structured_extraction",
                        &chat.structured_extraction_llm,
                    ),
                    chat.agent_max_tokens,
                    Arc::clone(&prompts),
                ))
            } else {
                None
            };

            let qg = if chat.quality_guard_enabled {
                Some(Arc::new(QualityGuard::new_with_prompts(
                    resolve_chat_agent_llm("quality_guard", &chat.quality_guard_llm),
                    chat.quality_guard_threshold,
                    max_tok.min(256),
                    Arc::clone(&prompts),
                )))
            } else {
                None
            };

            let la = if chat.language_adapter_enabled {
                Some(LanguageAdapter::new(
                    resolve_chat_agent_llm("language_adapter", &chat.language_adapter_llm),
                    max_tok,
                ))
            } else {
                None
            };

            let po = if chat.orchestrator_enabled {
                Some(PipelineOrchestrator::new_with_prompts(
                    Some(resolve_chat_agent_llm(
                        "orchestrator",
                        &chat.orchestrator_llm,
                    )),
                    max_tok.min(256),
                    chat.max_orchestrator_calls,
                    Arc::clone(&prompts),
                ))
            } else {
                Some(PipelineOrchestrator::new_with_prompts(
                    None,
                    0,
                    0,
                    Arc::clone(&prompts),
                ))
            };

            // Feature 1: Conversation Memory
            let cm = if chat.conversation_memory_enabled {
                Some(ConversationMemory::new_with_prompts(
                    resolve_chat_agent_llm("memory", &chat.memory_llm),
                    chat.memory_summary_max_tokens,
                    Arc::clone(&prompts),
                ))
            } else {
                None
            };

            // Feature 3: Tool Router
            let tr = if chat.tool_use_enabled {
                Some(ToolRouter::new_with_prompts(
                    resolve_chat_agent_llm("tool_use", &chat.tool_use_llm),
                    Arc::clone(&search_engine),
                    chat.tool_use_max_calls,
                    max_tok.min(256),
                    Arc::clone(&prompts),
                ))
            } else {
                None
            };

            // Feature 5: Self-RAG
            let sr = if chat.self_rag_enabled {
                Some(SelfRag::new_with_prompts(
                    resolve_chat_agent_llm("self_rag", &chat.self_rag_llm),
                    chat.self_rag_threshold,
                    max_tok.min(256),
                    Arc::clone(&prompts),
                ))
            } else {
                None
            };

            // Feature 6: Graph RAG
            let gr = if chat.graph_rag_enabled {
                Some(GraphRag::new_with_prompts(
                    resolve_chat_agent_llm("graph_rag", &chat.graph_rag_llm),
                    chat.graph_rag_max_entities,
                    chat.graph_rag_max_depth,
                    max_tok.min(512),
                    Arc::clone(&prompts),
                ))
            } else {
                None
            };

            // Feature 7: Corrective RAG
            let cr = if chat.crag_enabled {
                Some(CorrectiveRag::new_with_prompts(
                    resolve_chat_agent_llm("crag", &None), // uses shared LLM
                    chat.crag_relevance_threshold,
                    if chat.crag_web_search_url.is_empty() {
                        None
                    } else {
                        Some(chat.crag_web_search_url.clone())
                    },
                    chat.crag_max_web_results,
                    max_tok.min(256),
                    Arc::clone(&prompts),
                ))
            } else {
                None
            };

            // Feature 8: Speculative RAG
            let sp = if chat.speculative_rag_enabled {
                Some(SpeculativeRag::new_with_prompts(
                    resolve_chat_agent_llm("speculative_rag", &chat.speculative_rag_llm),
                    chat.speculative_candidates,
                    max_tok,
                    Arc::clone(&prompts),
                ))
            } else {
                None
            };

            // Feature 9: Map-Reduce RAG
            let mr = if chat.map_reduce_enabled {
                Some(MapReduceRag::new_with_prompts(
                    resolve_chat_agent_llm("map_reduce", &chat.map_reduce_llm),
                    chat.map_reduce_max_chunks,
                    max_tok.min(256),
                    max_tok,
                    Arc::clone(&prompts),
                ))
            } else {
                None
            };

            // Feature 10: RAGAS Evaluation
            let ragas = if chat.ragas_enabled {
                Some(Arc::new(RagasEvaluator::new(
                    resolve_chat_agent_llm("ragas", &chat.ragas_llm),
                    chat.ragas_sample_rate,
                    max_tok.min(256),
                )))
            } else {
                None
            };

            // Feature 11: Contextual Compression
            let compress = if chat.compression_enabled {
                Some(ContextualCompression::new_with_prompts(
                    resolve_chat_agent_llm("compression", &chat.compression_llm),
                    chat.compression_target_ratio,
                    max_tok,
                    Arc::clone(&prompts),
                ))
            } else {
                None
            };

            // Feature 12: Multi-modal RAG
            let mm = if chat.multimodal_enabled {
                Some(MultimodalRag::new_with_prompts(
                    resolve_chat_agent_llm("multimodal", &chat.multimodal_llm),
                    max_tok.min(256),
                    chat.multimodal_max_images,
                    Arc::clone(&prompts),
                ))
            } else {
                None
            };

            // Feature 13: RAPTOR
            let raptor = if chat.raptor_enabled {
                Some(Raptor::new_with_prompts(
                    resolve_chat_agent_llm("raptor", &chat.raptor_llm),
                    chat.raptor_max_depth,
                    chat.raptor_group_size,
                    max_tok.min(512),
                    Arc::clone(&prompts),
                ))
            } else {
                None
            };

            // Feature 14: ColBERT Late Interaction Reranking
            let colbert = if chat.colbert_enabled {
                Some(ColbertReranker::new_with_prompts(
                    resolve_chat_agent_llm("colbert", &chat.colbert_llm),
                    max_tok.min(256),
                    chat.colbert_top_n,
                    Arc::clone(&prompts),
                ))
            } else {
                None
            };

            // Feature 15: Active Learning
            let al = if chat.active_learning_enabled {
                Some(Arc::new(ActiveLearning::new(
                    chat.active_learning_min_interactions,
                    chat.active_learning_max_low_confidence,
                )))
            } else {
                None
            };

            // Feature 16: Live Source Retrieval
            let live_retrieval = if chat.live_retrieval_enabled {
                // Use mcp config for timeouts if available, otherwise sensible defaults
                let mcp_connect_timeout = std::time::Duration::from_secs(30);
                let mcp_read_timeout = std::time::Duration::from_secs(120);
                Some(LiveRetrieval::new(
                    resolve_chat_agent_llm("live_retrieval", &chat.live_retrieval_llm),
                    max_tok.min(512),
                    std::time::Duration::from_secs(chat.live_retrieval_timeout_secs),
                    chat.live_retrieval_max_connectors,
                    chat.live_retrieval_max_content_chars,
                    mcp_connect_timeout,
                    mcp_read_timeout,
                    Arc::clone(&prompts),
                ))
            } else {
                None
            };

            #[allow(clippy::type_complexity)]
            let connector_provider: Option<
                Arc<
                    dyn Fn(
                            &thairag_core::permission::AccessScope,
                        ) -> Vec<thairag_core::types::McpConnectorConfig>
                        + Send
                        + Sync,
                >,
            > = if chat.live_retrieval_enabled {
                km_store.as_ref().map(|store| {
                    let store = Arc::clone(store);
                    Arc::new(
                        move |scope: &thairag_core::permission::AccessScope| -> Vec<
                            thairag_core::types::McpConnectorConfig,
                        > {
                            scope
                                .workspace_ids
                                .iter()
                                .flat_map(|ws_id| {
                                    store.list_connectors_for_workspace(*ws_id)
                                })
                                .filter(|c| {
                                    c.status == thairag_core::types::ConnectorStatus::Active
                                })
                                .collect()
                        },
                    )
                        as Arc<
                            dyn Fn(
                                    &thairag_core::permission::AccessScope,
                                )
                                    -> Vec<thairag_core::types::McpConnectorConfig>
                                + Send
                                + Sync,
                        >
                })
            } else {
                None
            };

            let doc_resolver: Option<
                Arc<dyn Fn(thairag_core::types::DocId) -> Option<String> + Send + Sync>,
            > = km_store.as_ref().map(|store| {
                let store = Arc::clone(store);
                Arc::new(move |doc_id: thairag_core::types::DocId| {
                    store.get_document(doc_id).ok().map(|d| d.title)
                })
                    as Arc<dyn Fn(thairag_core::types::DocId) -> Option<String> + Send + Sync>
            });

            // Agentic doc-selection: workspace document catalogue (id + title).
            let doc_catalog_resolver: Option<thairag_agent::chat_pipeline::DocCatalogResolver> =
                km_store.as_ref().map(|store| {
                    let store = Arc::clone(store);
                    Arc::new(move |ws_ids: &[thairag_core::types::WorkspaceId]| {
                        ws_ids
                            .iter()
                            .flat_map(|ws| store.list_documents_in_workspace(*ws))
                            .filter(|d| d.status == thairag_core::models::DocStatus::Ready)
                            .map(|d| thairag_agent::doc_selector::CatalogEntry {
                                doc_id: d.id,
                                facets: d
                                    .processing_provenance
                                    .as_ref()
                                    .map(|p| p.facets.clone())
                                    .unwrap_or_default(),
                                title: d.title,
                            })
                            .collect()
                    }) as thairag_agent::chat_pipeline::DocCatalogResolver
                });

            // Doc-ops (pre-retrieval summarize): load a document's full stored
            // converted text — chunk contents joined in ingestion order.
            let doc_content_resolver: Option<thairag_agent::chat_pipeline::DocContentResolver> =
                km_store.as_ref().map(|store| {
                    let store = Arc::clone(store);
                    Arc::new(move |doc_id: thairag_core::types::DocId| {
                        let mut chunks = store.load_chunks_by_doc(doc_id);
                        if chunks.is_empty() {
                            return None;
                        }
                        chunks.sort_by_key(|c| c.chunk_index);
                        Some(
                            chunks
                                .iter()
                                .map(|c| c.content.as_str())
                                .collect::<Vec<_>>()
                                .join("\n\n"),
                        )
                    }) as thairag_agent::chat_pipeline::DocContentResolver
                });

            // PR-δ multimodal retrieval: resolve a chunk's image_blob_id to the
            // stored image bytes (base64) for the answer LLM's vision input. The
            // pipeline only invokes this when the answer LLM supports_vision().
            let image_resolver: Option<
                Arc<
                    dyn Fn(
                            thairag_core::types::ImageId,
                        ) -> Option<thairag_core::types::ImageContent>
                        + Send
                        + Sync,
                >,
            > = km_store.as_ref().map(|store| {
                let store = Arc::clone(store);
                Arc::new(move |img_id: thairag_core::types::ImageId| {
                    use base64::Engine;
                    store.get_image_blob(img_id).ok().flatten().map(|rec| {
                        thairag_core::types::ImageContent {
                            base64_data: base64::engine::general_purpose::STANDARD
                                .encode(&rec.bytes),
                            media_type: rec.mime,
                        }
                    })
                })
                    as Arc<
                        dyn Fn(
                                thairag_core::types::ImageId,
                            )
                                -> Option<thairag_core::types::ImageContent>
                            + Send
                            + Sync,
                    >
            });

            // Citation provenance: hydrate dropped ChunkMetadata (page numbers,
            // section title) from the store onto retrieval results, since the
            // vector/BM25 providers do not round-trip chunk metadata.
            let metadata_resolver: Option<thairag_agent::MetadataResolver> =
                km_store.as_ref().map(|store| {
                    let store = Arc::clone(store);
                    Arc::new(move |ids: &[String]| store.get_chunk_metadata(ids))
                        as thairag_agent::MetadataResolver
                });

            // Reasoning-based ("PageIndex") retriever for the Vectorless mode: an
            // LLM navigates per-document trees to the relevant sections. Store-
            // agnostic via the same resolver pattern as doc-selection — a tree
            // resolver (workspaces → DocTrees) and a chunk resolver (doc → chunks).
            let reasoning_retriever: Option<Arc<thairag_agent::ReasoningRetriever>> =
                km_store.as_ref().map(|store| {
                    let tree_store = Arc::clone(store);
                    let tree_resolver: thairag_agent::TreeResolver =
                        Arc::new(move |ws_ids: &[thairag_core::types::WorkspaceId]| {
                            tree_store
                                .list_document_trees(ws_ids)
                                .unwrap_or_default()
                                .into_iter()
                                .filter_map(|(_, json)| {
                                    serde_json::from_str::<thairag_core::models::DocTree>(&json)
                                        .ok()
                                })
                                .collect()
                        });
                    // Full section text (PageIndex feeds whole selected sections,
                    // not re-chunked pieces): resolve the doc's converted markdown
                    // so the retriever can slice the selected node's pages intact.
                    let content_store = Arc::clone(store);
                    let content_resolver: thairag_agent::ContentResolver =
                        Arc::new(move |doc_id: thairag_core::types::DocId| {
                            content_store.get_document_content(doc_id).ok().flatten()
                        });
                    let chunk_store = Arc::clone(store);
                    let chunk_resolver: thairag_agent::ChunkResolver =
                        Arc::new(move |doc_id: thairag_core::types::DocId| {
                            chunk_store.load_chunks_by_doc(doc_id)
                        });
                    let nav_llm = resolve_chat_agent_llm("reasoning_nav", &chat.reasoning_nav_llm);
                    Arc::new(thairag_agent::ReasoningRetriever::new(
                        nav_llm,
                        tree_resolver,
                        content_resolver,
                        chunk_resolver,
                        chat.reasoning_max_docs,
                        chat.reasoning_max_nodes,
                        chat.agent_max_tokens,
                    ))
                });

            // ── Guardrails (PR1): build only when respective master switch is on ──
            let input_guardrails = if chat.input_guardrails_enabled {
                Some(Arc::new(thairag_agent::guardrails::InputGuardrails::new(
                    chat.guardrails.clone(),
                )))
            } else {
                None
            };
            let output_guardrails = if chat.output_guardrails_enabled {
                Some(Arc::new(thairag_agent::guardrails::OutputGuardrails::new(
                    chat.guardrails.clone(),
                )))
            } else {
                None
            };

            let pipeline = ChatPipeline::new(
                Arc::clone(&llm),
                Arc::clone(&search_engine),
                qa,
                qr,
                cc,
                rg,
                se,
                qg,
                la,
                po,
                cm,
                tr,
                sr,
                gr,
                cr,
                sp,
                mr,
                ragas,
                compress,
                mm,
                raptor,
                colbert,
                al,
                live_retrieval,
                connector_provider,
                input_guardrails,
                output_guardrails,
                chat.clone(),
                Arc::clone(&prompts),
                doc_resolver,
                image_resolver,
            );
            let pipeline = match &plugin_engine {
                Some(engine) => pipeline.with_search_plugin_engine(Arc::clone(engine)),
                None => pipeline,
            };
            let pipeline = match &guardrail_metrics {
                Some(rec) => pipeline.with_guardrail_metrics(Arc::clone(rec)),
                None => pipeline,
            };
            let pipeline = match metadata_resolver {
                Some(resolver) => pipeline.with_metadata_resolver(resolver),
                None => pipeline,
            };
            let pipeline = match doc_catalog_resolver {
                Some(resolver) => pipeline.with_doc_catalog_resolver(resolver),
                None => pipeline,
            };
            let pipeline = match doc_content_resolver {
                Some(resolver) => pipeline.with_doc_content_resolver(resolver),
                None => pipeline,
            };
            let pipeline = match reasoning_retriever {
                Some(rr) => pipeline.with_reasoning_retriever(rr),
                None => pipeline,
            };
            Some(Arc::new(pipeline))
        } else {
            None
        };

        // ── Context Compaction ──
        let context_compactor = if chat.context_compaction_enabled {
            let compactor_llm = if let Some(ref cfg) = chat.personal_memory_llm {
                let resolved = if let Some(v) = vault {
                    resolve_profile(cfg, store_ref, v)
                } else {
                    cfg.clone()
                };
                Arc::from(create_llm_provider(&resolved))
            } else {
                Arc::clone(&llm)
            };
            Some(Arc::new(ContextCompactor::new_with_prompts(
                compactor_llm,
                chat.agent_max_tokens,
                Arc::clone(&prompts),
            )))
        } else {
            None
        };

        // ── Personal Memory (Per-User RAG) ──
        let personal_memory_manager = if chat.personal_memory_enabled {
            let pm_store =
                create_personal_memory_store(&providers.vector_store, embedding.dimension());
            Some(Arc::new(PersonalMemoryManager::new(
                Arc::clone(&embedding),
                pm_store,
                chat.personal_memory_top_k,
                chat.personal_memory_max_per_user,
            )))
        } else {
            None
        };

        Self {
            providers_config: providers.clone(),
            chat_pipeline_config: chat.clone(),
            orchestrator,
            chat_pipeline,
            document_pipeline,
            search_engine,
            embedding,
            image_embedding,
            reranker,
            context_compactor,
            personal_memory_manager,
            chat_llm: llm,
        }
    }
}

/// LLM10: Per-user concurrent request limiter to prevent resource exhaustion.
#[derive(Clone)]
pub struct UserRequestLimiter {
    /// Maps user_id → current in-flight request count.
    active: Arc<dashmap::DashMap<String, u32>>,
    /// Max concurrent requests per user.
    max_concurrent: u32,
}

impl UserRequestLimiter {
    pub fn new(max_concurrent: u32) -> Self {
        Self {
            active: Arc::new(dashmap::DashMap::new()),
            max_concurrent,
        }
    }

    /// Evict users with zero active requests.
    pub fn cleanup(&self) {
        self.active.retain(|_, count| *count > 0);
    }

    /// Try to acquire a request slot. Returns Err if limit exceeded.
    #[allow(clippy::result_unit_err)]
    pub fn try_acquire(&self, user_id: &str) -> Result<UserRequestGuard, ()> {
        let mut entry = self.active.entry(user_id.to_string()).or_insert(0);
        if *entry >= self.max_concurrent {
            return Err(());
        }
        *entry += 1;
        Ok(UserRequestGuard {
            active: Arc::clone(&self.active),
            user_id: user_id.to_string(),
        })
    }
}

/// RAII guard that decrements the counter when the request completes.
pub struct UserRequestGuard {
    active: Arc<dashmap::DashMap<String, u32>>,
    user_id: String,
}

impl Drop for UserRequestGuard {
    fn drop(&mut self) {
        if let Some(mut entry) = self.active.get_mut(&self.user_id) {
            *entry = entry.saturating_sub(1);
            if *entry == 0 {
                drop(entry);
                self.active.remove(&self.user_id);
            }
        }
    }
}

/// Cache of scoped pipelines, keyed by a hash of the effective config.
/// Generic over the pipeline type so the same TTL/eviction logic serves both
/// chat (`ChatPipeline`) and document (`DocumentPipeline`) pipelines. Entries
/// expire after `ttl` seconds to pick up settings changes without requiring
/// explicit invalidation.
pub struct ScopedPipelineCache<P> {
    cache: Arc<dashmap::DashMap<u64, (Arc<P>, Instant)>>,
    ttl: std::time::Duration,
}

// Hand-written so the bound is `P` (not `P: Clone`): we only clone the `Arc`s,
// never `P` itself, and the cached pipelines are not `Clone`.
impl<P> Clone for ScopedPipelineCache<P> {
    fn clone(&self) -> Self {
        Self {
            cache: Arc::clone(&self.cache),
            ttl: self.ttl,
        }
    }
}

impl<P> ScopedPipelineCache<P> {
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            cache: Arc::new(dashmap::DashMap::new()),
            ttl: std::time::Duration::from_secs(ttl_secs),
        }
    }

    /// Get a cached pipeline if it exists and hasn't expired.
    pub fn get(&self, key: u64) -> Option<Arc<P>> {
        if let Some(entry) = self.cache.get(&key) {
            if entry.1.elapsed() < self.ttl {
                return Some(Arc::clone(&entry.0));
            }
            drop(entry);
            self.cache.remove(&key);
        }
        None
    }

    /// Insert a pipeline into the cache.
    pub fn insert(&self, key: u64, pipeline: Arc<P>) {
        self.cache.insert(key, (pipeline, Instant::now()));
    }

    /// Invalidate all cached pipelines (e.g. after global settings change).
    pub fn clear(&self) {
        self.cache.clear();
    }
}

/// Hash a `ChatPipelineConfig` by serializing it to JSON, then hashing the bytes.
pub fn hash_chat_pipeline_config(cfg: &ChatPipelineConfig) -> u64 {
    let json = serde_json::to_string(cfg).unwrap_or_default();
    let mut hasher = DefaultHasher::new();
    json.hash(&mut hasher);
    hasher.finish()
}

/// Hash a `DocumentConfig` by serializing it to JSON, then hashing the bytes.
/// Keys the scoped document-pipeline cache so two scopes that resolve to the
/// same effective config share one pipeline.
pub fn hash_document_config(cfg: &DocumentConfig) -> u64 {
    let json = serde_json::to_string(cfg).unwrap_or_default();
    let mut hasher = DefaultHasher::new();
    json.hash(&mut hasher);
    hasher.finish()
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub jwt: Option<Arc<JwtService>>,
    pub api_keys: Arc<std::collections::HashSet<String>>,
    pub km_store: Arc<dyn KmStoreTrait>,
    pub session_store: Arc<dyn thairag_core::traits::SessionStoreTrait>,
    pub metrics: Arc<MetricsState>,
    pub oidc_state_cache: OidcStateCache,
    pub login_tracker: LoginTracker,
    pub prompt_registry: Arc<PromptRegistry>,
    pub user_request_limiter: UserRequestLimiter,
    /// Per-user token-bucket rate limiter (applied after auth).
    pub user_rate_limiter: crate::rate_limit::UserRateLimiter,
    /// Per-IP rate limiter reference (for stats dashboard). None if rate limiting disabled.
    pub ip_rate_limiter: Option<crate::rate_limit::RateLimiter>,
    pub vault: Arc<Vault>,
    pub embedding_cache: Arc<dyn thairag_core::traits::EmbeddingCache>,
    pub job_queue: Arc<dyn thairag_core::traits::JobQueue>,
    pub webhook_dispatcher: crate::webhook::WebhookDispatcher,
    pub plugin_registry: Arc<crate::plugin_registry::PluginRegistry>,
    pub training_runner: Arc<crate::training_runner::TrainingRunner>,
    providers: Arc<RwLock<ProviderBundle>>,
    pub scoped_pipeline_cache: ScopedPipelineCache<ChatPipeline>,
    /// Scoped document pipelines (org/dept/workspace overrides of chunking,
    /// PDF/OCR, and AI-preprocessing config) resolved at ingest time.
    pub scoped_doc_pipeline_cache: ScopedPipelineCache<DocumentPipeline>,
    migration_status: crate::vector_migration::SharedMigrationStatus,
    /// Cached, layered model-capability catalog driving advisory ⭐/vision badges.
    pub model_catalog: Arc<crate::model_catalog::ModelCatalog>,
}

impl AppState {
    /// Get a snapshot of the current dynamic providers.
    pub fn providers(&self) -> ProviderBundle {
        self.providers.read().unwrap().clone()
    }

    /// Get the shared migration status for vector store migration.
    pub fn migration_status(&self) -> crate::vector_migration::SharedMigrationStatus {
        self.migration_status.clone()
    }

    /// Hot-swap the dynamic providers with a new bundle.
    /// Also clears the scoped pipeline cache since global config changed.
    pub fn reload_providers(&self, bundle: ProviderBundle) {
        *self.providers.write().unwrap() = bundle;
        self.scoped_pipeline_cache.clear();
        self.scoped_doc_pipeline_cache.clear();
    }

    /// Resolve a workspace ID to a full `SettingsScope` by walking
    /// workspace → dept → org.  Returns `Global` on any lookup failure.
    pub fn resolve_scope_for_workspace(
        &self,
        workspace_id: thairag_core::types::WorkspaceId,
    ) -> crate::store::SettingsScope {
        let ws = match self.km_store.get_workspace(workspace_id) {
            Ok(ws) => ws,
            Err(_) => return crate::store::SettingsScope::Global,
        };
        let dept = match self.km_store.get_dept(ws.dept_id) {
            Ok(d) => d,
            Err(_) => return crate::store::SettingsScope::Global,
        };
        crate::store::SettingsScope::Workspace {
            org_id: dept.org_id,
            dept_id: ws.dept_id,
            workspace_id,
        }
    }

    /// Get a `ChatPipeline` appropriate for the given settings scope.
    ///
    /// - If scope is `Global` or has no overrides, returns the global pipeline.
    /// - Otherwise builds a scoped pipeline (or returns a cached one).
    pub fn get_scoped_pipeline(
        &self,
        scope: &crate::store::SettingsScope,
    ) -> Option<Arc<ChatPipeline>> {
        let global_bundle = self.providers();

        // Fast path: global scope → use global pipeline directly
        if matches!(scope, crate::store::SettingsScope::Global) {
            return global_bundle.chat_pipeline;
        }

        // Check if there are any overrides at this scope
        let override_keys = {
            let chain = scope.inheritance_chain();
            let mut has_overrides = false;
            for (st, sid) in &chain {
                if *st != "global" && !self.km_store.list_override_keys(st, sid).is_empty() {
                    has_overrides = true;
                    break;
                }
            }
            has_overrides
        };

        // No overrides → use global pipeline (zero overhead)
        if !override_keys {
            return global_bundle.chat_pipeline;
        }

        // Resolve effective scoped config
        let scoped_config = crate::routes::settings::get_effective_chat_pipeline_scoped(
            &self.config,
            &*self.km_store,
            scope,
        );

        // Check if scoped config is same as global (hash comparison)
        let scoped_hash = hash_chat_pipeline_config(&scoped_config);
        let global_hash = hash_chat_pipeline_config(&global_bundle.chat_pipeline_config);
        if scoped_hash == global_hash {
            return global_bundle.chat_pipeline;
        }

        // Check cache
        if let Some(cached) = self.scoped_pipeline_cache.get(scoped_hash) {
            return Some(cached);
        }

        // Build a new pipeline with the scoped config but shared infrastructure
        let eff_search =
            crate::routes::settings::build_effective_search_config(&self.config, &*self.km_store);
        let scoped_bundle =
            ProviderBundleBuilder::new(
                &global_bundle.providers_config,
                &eff_search,
                &self.config.document,
                &scoped_config,
                Arc::clone(&self.prompt_registry),
            )
            .with_km_store(Arc::clone(&self.km_store))
            .with_vault(&self.vault)
            .with_embedding_cache(Arc::clone(&self.embedding_cache))
            .with_plugin_engine(Arc::clone(&self.plugin_registry)
                as Arc<dyn thairag_core::traits::SearchPluginEngine>)
            .with_guardrail_metrics(Arc::clone(&self.metrics)
                as Arc<dyn thairag_core::traits::GuardrailMetricsRecorder>)
            .build();

        if let Some(ref pipeline) = scoped_bundle.chat_pipeline {
            self.scoped_pipeline_cache
                .insert(scoped_hash, Arc::clone(pipeline));
            tracing::info!(
                scope = ?scope,
                hash = scoped_hash,
                "Built and cached scoped chat pipeline"
            );
        }

        scoped_bundle.chat_pipeline
    }

    /// Get a `DocumentPipeline` appropriate for the given settings scope.
    ///
    /// Mirrors [`get_scoped_pipeline`](Self::get_scoped_pipeline): an org/dept/
    /// workspace with document overrides (chunking, PDF/OCR, AI-preprocessing,
    /// per-agent models) gets a pipeline built from its effective scoped config;
    /// scopes without overrides — or whose overrides resolve to the same config
    /// as global — share the global pipeline at zero cost. Built pipelines are
    /// cached (TTL) so a busy org doesn't rebuild per document.
    pub fn get_scoped_document_pipeline(
        &self,
        scope: &crate::store::SettingsScope,
    ) -> Arc<DocumentPipeline> {
        let global_bundle = self.providers();

        // Fast path: global scope → global pipeline directly.
        if matches!(scope, crate::store::SettingsScope::Global) {
            return global_bundle.document_pipeline;
        }

        // Any overrides anywhere in the inheritance chain?
        let has_overrides = {
            let chain = scope.inheritance_chain();
            chain.iter().any(|(st, sid)| {
                *st != "global" && !self.km_store.list_override_keys(st, sid).is_empty()
            })
        };
        if !has_overrides {
            return global_bundle.document_pipeline;
        }

        // Resolve effective scoped config and compare against global. If an org
        // set overrides that happen to equal the global values, reuse global.
        let scoped_doc = crate::routes::settings::build_effective_document_config_scoped(
            &self.config,
            &*self.km_store,
            scope,
        );
        let global_doc =
            crate::routes::settings::build_effective_document_config(&self.config, &*self.km_store);
        let scoped_hash = hash_document_config(&scoped_doc);
        if scoped_hash == hash_document_config(&global_doc) {
            return global_bundle.document_pipeline;
        }

        // Cache hit?
        if let Some(cached) = self.scoped_doc_pipeline_cache.get(scoped_hash) {
            return cached;
        }

        // Build a bundle with the scoped document config but shared infra (same
        // providers, search, chat config as global) and lift out its pipeline.
        let eff_search =
            crate::routes::settings::build_effective_search_config(&self.config, &*self.km_store);
        let scoped_bundle =
            ProviderBundleBuilder::new(
                &global_bundle.providers_config,
                &eff_search,
                &scoped_doc,
                &global_bundle.chat_pipeline_config,
                Arc::clone(&self.prompt_registry),
            )
            .with_km_store(Arc::clone(&self.km_store))
            .with_vault(&self.vault)
            .with_embedding_cache(Arc::clone(&self.embedding_cache))
            .with_plugin_engine(Arc::clone(&self.plugin_registry)
                as Arc<dyn thairag_core::traits::SearchPluginEngine>)
            .with_guardrail_metrics(Arc::clone(&self.metrics)
                as Arc<dyn thairag_core::traits::GuardrailMetricsRecorder>)
            .build();

        let pipeline = Arc::clone(&scoped_bundle.document_pipeline);
        self.scoped_doc_pipeline_cache
            .insert(scoped_hash, Arc::clone(&pipeline));
        tracing::info!(
            scope = ?scope,
            hash = scoped_hash,
            "Built and cached scoped document pipeline"
        );
        pipeline
    }

    /// Build a new `ProviderBundle` with doc-title resolver and prompt registry.
    pub fn build_provider_bundle(
        &self,
        providers: &ProvidersConfig,
        search: &SearchConfig,
        doc: &DocumentConfig,
        chat: &ChatPipelineConfig,
    ) -> ProviderBundle {
        ProviderBundleBuilder::new(
            providers,
            search,
            doc,
            chat,
            Arc::clone(&self.prompt_registry),
        )
        .with_km_store(Arc::clone(&self.km_store))
        .with_vault(&self.vault)
        .with_embedding_cache(Arc::clone(&self.embedding_cache))
        .with_plugin_engine(
            Arc::clone(&self.plugin_registry) as Arc<dyn thairag_core::traits::SearchPluginEngine>
        )
        .with_guardrail_metrics(
            Arc::clone(&self.metrics) as Arc<dyn thairag_core::traits::GuardrailMetricsRecorder>
        )
        .build()
    }

    /// Construct from pre-built parts (used in tests).
    pub fn from_parts(
        config: Arc<AppConfig>,
        jwt: Option<Arc<JwtService>>,
        km_store: Arc<dyn KmStoreTrait>,
        bundle: ProviderBundle,
    ) -> Self {
        let login_tracker = LoginTracker::new(
            config.auth.max_login_attempts,
            config.auth.lockout_duration_secs,
        );

        // Use a unique temporary directory per invocation to avoid race conditions in parallel tests
        let test_vault_dir = std::env::temp_dir().join(format!(
            "thairag-test-vault-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let vault = Arc::new(Vault::init(
            test_vault_dir.to_str().unwrap_or("/tmp/thairag-test-vault"),
        ));

        let metrics = Arc::new(MetricsState::new());
        let webhook_dispatcher =
            crate::webhook::WebhookDispatcher::new(Arc::clone(&km_store), Some(metrics.clone()));

        let plugin_registry = Arc::new(crate::plugin_registry::PluginRegistry::new());
        crate::builtin_plugins::register_builtin_plugins(&plugin_registry);

        Self {
            config,
            jwt,
            api_keys: Arc::new(std::collections::HashSet::new()),
            km_store,
            session_store: Arc::new(InMemorySessionStore::new()),
            metrics,
            oidc_state_cache: OidcStateCache::new(),
            login_tracker,
            prompt_registry: Arc::new(PromptRegistry::new()),
            user_request_limiter: UserRequestLimiter::new(5),
            user_rate_limiter: crate::rate_limit::UserRateLimiter::new(10, 20),
            ip_rate_limiter: None,
            vault,
            embedding_cache: Arc::new(NoopEmbeddingCache),
            job_queue: Arc::new(crate::job_queue::InMemoryJobQueue::new()),
            webhook_dispatcher,
            plugin_registry,
            training_runner: Arc::new(crate::training_runner::TrainingRunner::new()),
            providers: Arc::new(RwLock::new(bundle)),
            scoped_pipeline_cache: ScopedPipelineCache::new(60),
            scoped_doc_pipeline_cache: ScopedPipelineCache::new(60),
            migration_status: Arc::new(tokio::sync::RwLock::new(
                crate::vector_migration::MigrationStatus::default(),
            )),
            model_catalog: Arc::new(crate::model_catalog::ModelCatalog::new()),
        }
    }

    pub async fn build(config: AppConfig) -> Self {
        // Load prompt registry early so it's available during provider build
        let prompt_registry = Arc::new(PromptRegistry::new());
        let prompts_dir = std::path::Path::new("prompts");
        match prompt_registry.load_from_dir(prompts_dir) {
            Ok(count) => {
                if count > 0 {
                    tracing::info!(
                        count,
                        "Loaded prompt templates from {}",
                        prompts_dir.display()
                    );
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to load prompt templates from {}", prompts_dir.display());
            }
        }

        let jwt = if config.auth.enabled {
            Some(Arc::new(JwtService::new(
                &config.auth.jwt_secret,
                config.auth.token_expiry_hours,
            )))
        } else {
            None
        };

        let km_store = create_km_store(&config.database.url, config.database.max_connections);

        // ── Session store backend selection ──
        let session_store: Arc<dyn thairag_core::traits::SessionStoreTrait> = match config
            .session
            .backend
            .as_str()
        {
            "redis" => {
                match thairag_provider_redis::RedisConnection::new(&config.redis.url).await {
                    Ok(conn) => {
                        tracing::info!("Session store: Redis");
                        Arc::new(thairag_provider_redis::RedisSessionStore::new(
                            conn,
                            config.session.max_history,
                            config.session.stale_timeout_secs,
                        ))
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to connect to Redis for sessions, falling back to memory");
                        Arc::new(InMemorySessionStore::new())
                    }
                }
            }
            _ => {
                tracing::info!("Session store: in-memory");
                Arc::new(InMemorySessionStore::new())
            }
        };
        let metrics = Arc::new(MetricsState::new());
        let oidc_state_cache = OidcStateCache::new();
        let login_tracker = LoginTracker::new(
            config.auth.max_login_attempts,
            config.auth.lockout_duration_secs,
        );

        // Apply KV store overrides to prompt registry (prompt.{key} entries)
        for (key, entry) in prompt_registry.list() {
            if let Some(override_val) = km_store.get_setting(&format!("prompt.{key}")) {
                prompt_registry.set(&key, override_val, entry.description, entry.category);
            }
        }
        // Also load prompts that exist only in KV store (no file counterpart)
        if let Some(all_prompt_keys) = km_store.get_setting("prompt._index") {
            for key in all_prompt_keys.split(',') {
                let key = key.trim();
                if !key.is_empty()
                    && prompt_registry.get(key).is_none()
                    && let Some(template) = km_store.get_setting(&format!("prompt.{key}"))
                {
                    let desc = km_store
                        .get_setting(&format!("prompt.{key}.description"))
                        .unwrap_or_default();
                    let cat = key.split('.').next().unwrap_or("chat").to_string();
                    prompt_registry.set(key, template, desc, cat);
                }
            }
        }

        // LLM10: Per-user concurrent request limiter (max 5 concurrent per user)
        let user_request_limiter = UserRequestLimiter::new(5);

        // Per-user token-bucket rate limiter (same config as per-IP)
        let user_rate_limiter = crate::rate_limit::UserRateLimiter::new(
            config.server.rate_limit.requests_per_second,
            config.server.rate_limit.burst_size,
        );

        // Parse static API keys from config
        let api_keys: std::collections::HashSet<String> = config
            .auth
            .api_keys
            .split(',')
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
            .collect();
        if !api_keys.is_empty() {
            tracing::info!(count = api_keys.len(), "Loaded static API keys");
        }

        let data_dir = std::env::var("THAIRAG_DATA_DIR").unwrap_or_else(|_| {
            if std::path::Path::new("/data").is_dir() {
                "/data".to_string()
            } else {
                "./data".to_string()
            }
        });
        let vault = Arc::new(Vault::init(&data_dir));

        // ── Embedding cache backend selection ──
        let embedding_cache: Arc<dyn thairag_core::traits::EmbeddingCache> = match config
            .embedding_cache
            .backend
            .as_str()
        {
            "redis" => {
                match thairag_provider_redis::RedisConnection::new(&config.redis.url).await {
                    Ok(conn) => {
                        tracing::info!("Embedding cache: Redis");
                        Arc::new(thairag_provider_redis::RedisEmbeddingCache::new(
                            conn,
                            config.embedding_cache.ttl_secs,
                        ))
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to connect to Redis for embedding cache, falling back to memory");
                        Arc::new(InMemoryEmbeddingCache::new(
                            config.embedding_cache.max_entries,
                            config.embedding_cache.ttl_secs,
                        ))
                    }
                }
            }
            "none" => {
                tracing::info!("Embedding cache: disabled");
                Arc::new(NoopEmbeddingCache)
            }
            _ => {
                tracing::info!(
                    "Embedding cache: in-memory (max_entries={}, ttl={}s)",
                    config.embedding_cache.max_entries,
                    config.embedding_cache.ttl_secs
                );
                Arc::new(InMemoryEmbeddingCache::new(
                    config.embedding_cache.max_entries,
                    config.embedding_cache.ttl_secs,
                ))
            }
        };

        // ── Job queue backend selection ──
        let job_queue: Arc<dyn thairag_core::traits::JobQueue> = match config
            .job_queue
            .backend
            .as_str()
        {
            "redis" => {
                match thairag_provider_redis::RedisConnection::new(&config.redis.url).await {
                    Ok(conn) => {
                        tracing::info!("Job queue: Redis");
                        Arc::new(thairag_provider_redis::RedisJobQueue::new(
                            conn,
                            config.job_queue.retention_secs,
                        ))
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to connect to Redis for job queue, falling back to memory");
                        Arc::new(crate::job_queue::InMemoryJobQueue::new())
                    }
                }
            }
            _ => {
                tracing::info!("Job queue: in-memory");
                Arc::new(crate::job_queue::InMemoryJobQueue::new())
            }
        };

        // ── Plugin registry ──
        // Built before the provider bundle so the chat pipeline can install
        // the registry as its SearchPluginEngine at construction time.
        let plugin_registry = Arc::new(crate::plugin_registry::PluginRegistry::new());
        crate::builtin_plugins::register_builtin_plugins(&plugin_registry);

        // Apply enabled plugins from KV store (if saved) or config defaults
        if let Some(saved) = km_store.get_setting("plugins.enabled") {
            let names: Vec<String> = saved.split(',').map(|s| s.trim().to_string()).collect();
            plugin_registry.set_enabled_plugins(&names);
            tracing::info!(
                count = names.len(),
                "Loaded plugin enabled state from KV store"
            );
        } else if !config.plugins.enabled_plugins.is_empty() {
            plugin_registry.set_enabled_plugins(&config.plugins.enabled_plugins);
            tracing::info!(
                count = config.plugins.enabled_plugins.len(),
                "Loaded plugin enabled state from config"
            );
        }

        // Re-build the provider bundle now that km_store is available, so DB-stored
        // per-agent LLM configs (from presets) are picked up on restart.
        let bundle = {
            let eff_chat = crate::routes::settings::get_effective_chat_pipeline_with_store(
                &config, &*km_store,
            );
            // Re-apply km_store overrides to the document config too, so DB-stored
            // ai_preprocessing.* settings (e.g. AI preprocessing enabled) survive a
            // restart instead of reverting to the static file defaults.
            let eff_doc =
                crate::routes::settings::build_effective_document_config(&config, &*km_store);
            let eff_search =
                crate::routes::settings::build_effective_search_config(&config, &*km_store);
            // Also read DB-overridden provider config
            let pc = if let Some(json) = km_store.get_setting("provider_config")
                && let Ok(pc) = serde_json::from_str::<ProvidersConfig>(&json)
            {
                pc
            } else {
                config.providers.clone()
            };
            ProviderBundleBuilder::new(
                &pc,
                &eff_search,
                &eff_doc,
                &eff_chat,
                Arc::clone(&prompt_registry),
            )
            .with_km_store(Arc::clone(&km_store))
            .with_vault(&vault)
            .with_embedding_cache(Arc::clone(&embedding_cache))
            .with_plugin_engine(
                Arc::clone(&plugin_registry) as Arc<dyn thairag_core::traits::SearchPluginEngine>
            )
            .with_guardrail_metrics(
                Arc::clone(&metrics) as Arc<dyn thairag_core::traits::GuardrailMetricsRecorder>
            )
            .build()
        };

        // Store the embedding fingerprint on startup so snapshot restore
        // can detect mismatches even if no manual change has been made.
        let emb_fp = format!(
            "{:?}:{}:{}",
            bundle.providers_config.embedding.kind,
            bundle.providers_config.embedding.model,
            bundle.providers_config.embedding.dimension,
        );
        km_store.set_setting("_embedding_fingerprint", &emb_fp);

        let webhook_dispatcher =
            crate::webhook::WebhookDispatcher::new(Arc::clone(&km_store), Some(metrics.clone()));

        // Recover interrupted finetune jobs
        let training_runner = Arc::new(crate::training_runner::TrainingRunner::new());
        crate::training_runner::TrainingRunner::recover_interrupted_jobs(&*km_store);

        Self {
            config: Arc::new(config),
            jwt,
            api_keys: Arc::new(api_keys),
            km_store,
            session_store,
            metrics,
            oidc_state_cache,
            login_tracker,
            prompt_registry,
            user_request_limiter,
            user_rate_limiter,
            ip_rate_limiter: None,
            vault,
            embedding_cache,
            job_queue,
            webhook_dispatcher,
            plugin_registry,
            training_runner,
            providers: Arc::new(RwLock::new(bundle)),
            scoped_pipeline_cache: ScopedPipelineCache::new(60),
            scoped_doc_pipeline_cache: ScopedPipelineCache::new(60),
            migration_status: Arc::new(tokio::sync::RwLock::new(
                crate::vector_migration::MigrationStatus::default(),
            )),
            model_catalog: Arc::new(crate::model_catalog::ModelCatalog::new()),
        }
    }
}
