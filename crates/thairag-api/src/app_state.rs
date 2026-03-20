use std::sync::{Arc, RwLock};

use thairag_agent::active_learning::ActiveLearning;
use thairag_agent::colbert_reranker::ColbertReranker;
use thairag_agent::context_compactor::ContextCompactor;
use thairag_agent::context_curator::ContextCurator;
use thairag_agent::contextual_compression::ContextualCompression;
use thairag_agent::conversation_memory::ConversationMemory;
use thairag_agent::corrective_rag::CorrectiveRag;
use thairag_agent::graph_rag::GraphRag;
use thairag_agent::language_adapter::LanguageAdapter;
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
use thairag_agent::tool_router::ToolRouter;
use thairag_agent::{ChatPipeline, PipelineOrchestrator, QueryOrchestrator, RagEngine};
use thairag_auth::JwtService;
use thairag_config::AppConfig;
use thairag_config::schema::{ChatPipelineConfig, DocumentConfig, ProvidersConfig, SearchConfig};
use thairag_core::PromptRegistry;
use thairag_core::traits::{EmbeddingModel, LlmProvider, Reranker, TextSearch, VectorStore};
use thairag_document::DocumentPipeline;
use thairag_search::HybridSearchEngine;

use thairag_provider_embedding::create_embedding_provider_with_options;
use thairag_provider_llm::{create_llm_provider, create_llm_provider_with_options};
use thairag_provider_reranker::create_reranker;
use thairag_provider_search::create_text_search;
use thairag_provider_vectordb::{create_personal_memory_store, create_vector_store};

use crate::login_tracker::LoginTracker;
use crate::metrics::MetricsState;
use crate::oidc::OidcStateCache;
use crate::session::SessionStore;
use crate::store::{KmStoreTrait, create_km_store};

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
    pub context_compactor: Option<Arc<ContextCompactor>>,
    pub personal_memory_manager: Option<Arc<PersonalMemoryManager>>,
}

impl ProviderBundle {
    pub fn build(
        providers: &ProvidersConfig,
        search: &SearchConfig,
        doc: &DocumentConfig,
        chat: &ChatPipelineConfig,
    ) -> Self {
        Self::build_with_prompts(
            providers,
            search,
            doc,
            chat,
            Arc::new(thairag_core::PromptRegistry::new()),
        )
    }

    pub fn build_with_prompts(
        providers: &ProvidersConfig,
        search: &SearchConfig,
        doc: &DocumentConfig,
        chat: &ChatPipelineConfig,
        prompts: Arc<thairag_core::PromptRegistry>,
    ) -> Self {
        let ollama_ka = &chat.ollama_keep_alive;
        let ka_opt = if ollama_ka.is_empty() {
            None
        } else {
            Some(ollama_ka.as_str())
        };
        let llm: Arc<dyn LlmProvider> = Arc::from(create_llm_provider_with_options(
            &providers.llm,
            120,
            ka_opt,
        ));
        let embedding: Arc<dyn EmbeddingModel> = Arc::from(create_embedding_provider_with_options(
            &providers.embedding,
            ka_opt,
        ));
        let vector_store: Arc<dyn VectorStore> =
            Arc::from(create_vector_store(&providers.vector_store));
        let text_search: Arc<dyn TextSearch> =
            Arc::from(create_text_search(&providers.text_search));
        let reranker: Arc<dyn Reranker> = Arc::from(create_reranker(&providers.reranker));

        let search_engine = Arc::new(HybridSearchEngine::new(
            Arc::clone(&embedding),
            vector_store,
            text_search,
            reranker,
            search.clone(),
        ));

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
        let shared_preprocessing_llm: Arc<dyn LlmProvider> =
            if let Some(ref cfg) = doc.ai_preprocessing.llm {
                Arc::from(create_llm_provider(cfg))
            } else {
                Arc::clone(&llm)
            };

        let resolve_agent_llm =
            |agent_cfg: &Option<thairag_config::schema::LlmConfig>| -> Arc<dyn LlmProvider> {
                if let Some(cfg) = agent_cfg {
                    Arc::from(create_llm_provider(cfg))
                } else {
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

        let document_pipeline = Arc::new(DocumentPipeline::new_with_per_agent_ai_and_prompts(
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
        ));

        // ── Chat Pipeline (multi-agent) ──
        let chat_pipeline = if chat.enabled {
            let chat_timeout = chat.request_timeout_secs;
            let chat_shared_llm: Arc<dyn LlmProvider> = if let Some(ref cfg) = chat.llm {
                Arc::from(create_llm_provider_with_options(cfg, chat_timeout, ka_opt))
            } else {
                Arc::clone(&llm)
            };

            let resolve_chat_agent_llm = |agent_name: &str,
                                          agent_cfg: &Option<thairag_config::schema::LlmConfig>|
             -> Arc<dyn LlmProvider> {
                if let Some(cfg) = agent_cfg {
                    tracing::info!(
                        agent = agent_name,
                        kind = ?cfg.kind,
                        model = %cfg.model,
                        "Chat agent: using per-agent LLM"
                    );
                    Arc::from(create_llm_provider_with_options(cfg, chat_timeout, ka_opt))
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

            let rg = ResponseGenerator::new_with_prompts(
                resolve_chat_agent_llm("response_generator", &chat.response_generator_llm),
                Arc::clone(&prompts),
            );

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
                    Arc::clone(&chat_shared_llm),
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

            Some(Arc::new(ChatPipeline::new(
                Arc::clone(&llm),
                Arc::clone(&search_engine),
                qa,
                qr,
                cc,
                rg,
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
                chat.clone(),
                Arc::clone(&prompts),
            )))
        } else {
            None
        };

        // ── Context Compaction ──
        let context_compactor = if chat.context_compaction_enabled {
            let compactor_llm = if let Some(ref cfg) = chat.personal_memory_llm {
                Arc::from(create_llm_provider(cfg))
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
            context_compactor,
            personal_memory_manager,
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

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub jwt: Option<Arc<JwtService>>,
    pub api_keys: Arc<std::collections::HashSet<String>>,
    pub km_store: Arc<dyn KmStoreTrait>,
    pub session_store: Arc<SessionStore>,
    pub metrics: Arc<MetricsState>,
    pub oidc_state_cache: OidcStateCache,
    pub login_tracker: LoginTracker,
    pub prompt_registry: Arc<PromptRegistry>,
    pub user_request_limiter: UserRequestLimiter,
    /// Per-user token-bucket rate limiter (applied after auth).
    pub user_rate_limiter: crate::rate_limit::UserRateLimiter,
    providers: Arc<RwLock<ProviderBundle>>,
}

impl AppState {
    /// Get a snapshot of the current dynamic providers.
    pub fn providers(&self) -> ProviderBundle {
        self.providers.read().unwrap().clone()
    }

    /// Hot-swap the dynamic providers with a new bundle.
    pub fn reload_providers(&self, bundle: ProviderBundle) {
        *self.providers.write().unwrap() = bundle;
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
        Self {
            config,
            jwt,
            api_keys: Arc::new(std::collections::HashSet::new()),
            km_store,
            session_store: Arc::new(SessionStore::new()),
            metrics: Arc::new(MetricsState::new()),
            oidc_state_cache: OidcStateCache::new(),
            login_tracker,
            prompt_registry: Arc::new(PromptRegistry::new()),
            user_request_limiter: UserRequestLimiter::new(5),
            user_rate_limiter: crate::rate_limit::UserRateLimiter::new(10, 20),
            providers: Arc::new(RwLock::new(bundle)),
        }
    }

    pub fn build(config: AppConfig) -> Self {
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

        let bundle = ProviderBundle::build_with_prompts(
            &config.providers,
            &config.search,
            &config.document,
            &config.chat_pipeline,
            Arc::clone(&prompt_registry),
        );

        let jwt = if config.auth.enabled {
            Some(Arc::new(JwtService::new(
                &config.auth.jwt_secret,
                config.auth.token_expiry_hours,
            )))
        } else {
            None
        };

        let km_store = create_km_store(&config.database.url, config.database.max_connections);
        let session_store = Arc::new(SessionStore::new());
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
            providers: Arc::new(RwLock::new(bundle)),
        }
    }
}
