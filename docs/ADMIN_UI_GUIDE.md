# Admin UI Guide

The ThaiRAG Admin UI is a React + Ant Design application for managing the entire ThaiRAG platform. It runs on port 8081 (Docker) or 5173 (dev server).

## Table of Contents

1. [Login](#login)
2. [Dashboard](#dashboard)
3. [KM Hierarchy](#km-hierarchy)
4. [Documents](#documents)
5. [Test Chat](#test-chat)
6. [Users](#users)
7. [Permissions](#permissions)
8. [Usage & Costs](#usage--costs)
9. [Feedback & Tuning](#feedback--tuning)
10. [Settings](#settings)
11. [Health](#health)
12. [Config Snapshots](#config-snapshots)
13. [Collapsible Settings](#collapsible-settings)

## Access Control

Pages are role-gated:

| Role | Accessible Pages |
|------|-----------------|
| `viewer` | Dashboard, Health |
| `editor` | + KM Hierarchy, Documents, Test Chat |
| `admin` | + Users, Permissions, Usage & Costs, Feedback & Tuning |
| `super_admin` | + Settings |

The sidebar menu automatically shows only pages the logged-in user can access.

---

## Login

**Path:** `/login`

Standard email/password login form. On first use, register a new account — the first user automatically becomes a super admin.

If external identity providers are configured (OIDC, OAuth2, SAML, LDAP), their buttons appear below the local login form under a "or sign in with" divider.

- **OIDC/OAuth2/SAML** providers show as redirect buttons
- **LDAP** providers show an inline username/password form

---

## Dashboard

**Path:** `/` | **Min role:** `viewer`

Overview page showing system status at a glance:

- **KM Statistics** — Total organizations, departments, workspaces, documents, chunks
- **System Status** — API health, provider connectivity
- **Recent Activity** — Latest document uploads and user actions
- **Quick Actions** — Links to common tasks (upload document, create workspace)

---

## KM Hierarchy

**Path:** `/km` | **Min role:** `editor`

Manage the knowledge management hierarchy: **Organizations → Departments → Workspaces**.

### Organizations
- **Create** — Click "Add Organization", enter name
- **View** — Click an org to see its departments
- **Delete** — Delete button with confirmation (cascades to departments and workspaces)

### Departments
- **Create** — Within an org, click "Add Department"
- **View** — Click a department to see its workspaces
- **Delete** — Removes department and all child workspaces

### Workspaces
- **Create** — Within a department, click "Add Workspace"
- **View** — Shows workspace details and document count
- **Delete** — Removes workspace and all associated documents

The page uses a tree/list layout where you drill down through the hierarchy.

---

## Documents

**Path:** `/documents` | **Min role:** `editor`

Manage documents within workspaces.

### Document List
- Select a workspace from the dropdown to view its documents
- Table shows: Title, Format, Size, Chunks, Status, Created date
- Click a document to view its content and chunk details

### Upload
- Click "Upload Document" to open the upload modal
- **Supported formats:** PDF, DOCX, XLSX, HTML, Markdown, CSV, plain text
- Documents are automatically converted, chunked, embedded, and indexed
- Upload size limit is configurable (default varies by tier)

### Document Actions
- **View Content** — See the extracted text content
- **View Chunks** — Browse individual chunks with their metadata (page numbers, section titles, chunk index)
- **Download** — Download the original file
- **Reprocess** — Re-chunk and re-embed the document (useful after changing chunk settings)
- **Delete** — Remove document and all its chunks from vector DB and search index

---

## Test Chat

**Path:** `/test-chat` | **Min role:** `editor`

Interactive chat interface for testing RAG responses against specific workspaces.

### Chat Interface
1. Select a workspace from the dropdown
2. Type a query and press Enter or click Send
3. The response shows:
   - Generated answer from the RAG pipeline
   - Retrieved chunks with relevance scores
   - Timing breakdown (search time, generation time, total)
   - Token usage (prompt + completion)
   - Provider info (LLM model, embedding model)

### Feedback Controls
Each response has three action buttons:
- **Thumbs Up** — Mark response as good quality (turns green when active)
- **Thumbs Down** — Mark response as poor quality, opens a comment modal for details
- **Star** — Save the Q&A pair as a golden example for few-shot learning

Feedback is stored with full context (query, answer, retrieved chunks, scores, workspace ID) and drives the auto-tuning system.

### Pipeline Stages
Each response includes a collapsible pipeline stages panel showing exactly which agents ran, how long each took, and which models were used. See [Pipeline Stages UI](#pipeline-stages-ui) for details.

### Chat Persistence
Chat history and workspace selection persist across page navigation within the same tab. See [Chat Persistence](#chat-persistence) for details.

### Session Management
- Each chat session maintains conversation history (up to 50 messages)
- Sessions auto-expire after 1 hour of inactivity
- Start a new session by refreshing or selecting a different workspace
- **Context compaction**: When enabled, long conversations are automatically compacted — older messages are summarized and recent messages kept intact, so the user can continue chatting without hitting context limits
- **Personal memory**: When enabled, the system remembers user preferences, facts, and decisions across sessions via per-user vector storage

---

## Users

**Path:** `/users` | **Min role:** `admin`

Manage platform users.

### User Table
Columns:
- **Email** — User's email address
- **Name** — Display name
- **Role** — `viewer`, `editor`, `admin`, or `super_admin`
- **Provider** — Auth provider shown as a colored tag:
  - Blue: `local`
  - Green: `oidc`
  - Purple: `oauth2`
  - Orange: `saml`
  - Cyan: `ldap`
- **Super Admin** — Badge shown for super admin users
- **Created** — Registration date
- **Actions** — Delete button (disabled for super admin accounts)

### Deleting Users
- Click the delete button and confirm via the popover
- Super admin users cannot be deleted (button is disabled)
- Deleting a user revokes all their workspace permissions

---

## Permissions

**Path:** `/permissions` | **Min role:** `admin`

Manage workspace access permissions for users.

### Permission Levels
Permissions can be granted at three levels, each cascading downward:
- **Organization level** — Grants access to all workspaces in all departments
- **Department level** — Grants access to all workspaces in the department
- **Workspace level** — Grants access to a single workspace

### Managing Permissions
1. Select the scope level (Organization, Department, or Workspace)
2. Select the specific entity
3. View current permissions in the table
4. **Grant** — Select a user and click "Grant Access"
5. **Revoke** — Click the revoke button next to an existing permission

---

## Usage & Costs

**Path:** `/usage` | **Min role:** `admin`

Monitor API usage and estimate costs.

### Usage Statistics
- **Total Tokens** — Cumulative prompt and completion tokens
- **Request Counts** — Total API requests by endpoint
- **Cost Estimation** — Estimated costs based on provider pricing:
  - LLM tokens (prompt/completion rates vary by model)
  - Embedding tokens
  - Reranker API calls

### Provider Info
Shows the current provider configuration for cost context:
- LLM provider and model
- Embedding provider and model
- This helps admins understand cost implications

Usage data persists across server restarts via the KV store.

---

## Feedback & Tuning

**Path:** `/feedback` | **Min role:** `admin`

The feedback-driven auto-tuning dashboard with five tabs.

### Overview Tab
- **Stats Cards** — Total feedback, positive count, negative count, satisfaction rate
- **Quality Guard Threshold** — Current adaptive threshold based on feedback ratio
- **Auto-Tuning Status** — Whether document boosts and retrieval adjustments are active

### Entries Tab
- Paginated log of all feedback entries
- **Filter** — All, Positive only, Negative only
- **Workspace Filter** — Filter by workspace
- Each entry shows: timestamp, query, thumbs rating, workspace
- **Expandable rows** — Click to see the full answer and chunk scores

### Document Boosts Tab
- Table showing per-document boost/penalty multipliers
- Columns: Document ID, Boost (percentage), Positive count, Negative count
- Boost range: 50% to 150% (requires minimum 3 feedback samples)
- Documents with mostly positive feedback get boosted in search results
- Documents with mostly negative feedback get penalized

### Golden Examples Tab
- Table of curated Q&A pairs used for few-shot learning
- Columns: Query, Answer (truncated), Workspace, Created date
- **Delete** — Remove an example with confirmation
- **Expandable rows** — View full answer text
- Up to 5 examples are injected per query (workspace-specific + global)
- Maximum 100 golden examples stored

### Retrieval Tuning Tab
- **Auto-Suggestions** — When enough feedback data exists, the system suggests parameter adjustments. Click "Apply" to accept.
- **Parameter Controls:**
  - `top_k` — Number of chunks to retrieve (InputNumber, 1-50)
  - `min_score_threshold` — Minimum relevance score to include a chunk (Slider, 0-1)
  - `vector_weight` — Weight for vector search in RRF (Slider, 0-1)
  - `bm25_weight` — Weight for BM25 search in RRF (Slider, 0-1)
- **Save** — Persist changes (applied to the next query)
- **Reset** — Revert to default parameters

---

## Settings

**Path:** `/settings` | **Min role:** `super_admin`

System configuration for super administrators. Contains multiple tabs.

### Identity Providers Tab
Manage external authentication providers:

- **Table** — Shows all configured identity providers with columns: Name, Type (Tag), Enabled (Tag), Created, Actions
- **Add Provider** — Opens a form modal with:
  - Name, Type (OIDC, OAuth2, SAML, LDAP), Enabled toggle
  - Dynamic config fields based on type:
    - **OIDC:** Issuer URL, Client ID, Client Secret, Scopes, Redirect URI
    - **OAuth2:** Authorize URL, Token URL, UserInfo URL, Client ID, Client Secret, Scopes
    - **SAML:** IdP Entity ID, SSO URL, SLO URL, Certificate, SP Entity ID
    - **LDAP:** Server URL, Bind DN, Bind Password, Search Base, Search Filter, TLS toggle
  - Secrets are rendered as password inputs
- **Test** — Test connectivity to the provider (returns success/failure)
- **Edit** — Modify provider configuration
- **Delete** — Remove provider with confirmation

### Provider Configuration Tab
Configure the AI provider stack at runtime:
- **LLM** — Provider type (Claude/OpenAI/Ollama), model selection, API key
- **Embeddings** — Provider type, model, dimension
- **Reranker** — Provider type, model
- **Model Sync** — Fetch available models from configured providers
- **Presets** — Quick-apply preset configurations (free, standard, premium)

### Document Processing Tab
Configure document ingestion parameters:
- Chunk size and overlap
- Maximum upload size

### Chat Pipeline Tab
Configure the RAG pipeline behavior:
- System prompt customization
- Guardrail settings
- Pre/post processor configuration

#### LLM Mode

Controls how LLMs are assigned to pipeline agents:

| Mode | Behavior |
|------|----------|
| **Use Chat LLM** | All agents use the main LLM Provider directly (simplest setup) |
| **Shared** | All agents share a single dedicated chat LLM (can be different from the main LLM Provider) |
| **Per-Agent** | Each agent can have its own LLM with individual model selection |

**Per-Agent mode** allows fine-grained control — assign lightweight models (e.g., `qwen3:4b`) to simple agents like Query Analyzer, and heavier models (e.g., `qwen3:14b`) to Response Generator. Each agent panel header shows a model tag indicating which model it uses:

- **Purple tag** (e.g., `ollama: qwen3:14b`) — Agent has a dedicated model configured
- **Warning tag** (`No model (uses fallback)`) — Agent falls back through the chain: Shared chat LLM → main LLM Provider

The same model tags appear in the **Advanced Features** and **Next-Gen RAG** sections, showing which LLM each feature uses.

**Fallback chain:** Per-agent config → Shared chat LLM → LLM Provider section setting.

#### Agents

Toggle individual pipeline agents on/off:
- **Query Analyzer** — Analyzes and rewrites user queries for better retrieval
- **Retriever** — Searches the knowledge base for relevant chunks
- **Context Builder** — Assembles retrieved chunks into context
- **Response Generator** — Generates the final answer using the assembled context
- **Quality Checker** — Validates response quality and relevance
- **Guardrails** — Applies safety and compliance checks
- **Citation Manager** — Adds source citations to responses

**Advanced Features** section (inside collapsible panels):

#### Context Compaction
Automatic summarization of older messages when conversations approach the model's context window limit. Works like Claude Code's context compaction — users can continue chatting seamlessly without losing context.
- **Enabled** (Switch) — Turn on/off context compaction
- **Model Context Window** (InputNumber) — Context window size in tokens (0 = auto-detect from model)
- **Compaction Threshold** (InputNumber, 0.0–1.0) — Trigger compaction when token usage exceeds this fraction of context window (default: 0.8)
- **Keep Recent Messages** (InputNumber) — Number of recent messages to keep intact during compaction (default: 6)

#### Personal Memory
Per-user memory that persists across sessions. The system extracts typed memories (preferences, facts, decisions, corrections) from conversations and retrieves relevant ones for future chats — giving each user a personalized experience.
- **Enabled** (Switch) — Turn on/off personal memory
- **Top K** (InputNumber) — Number of memories to retrieve per query (default: 5)
- **Max Per User** (InputNumber) — Maximum memories stored per user (default: 200)
- **Decay Factor** (InputNumber, 0.0–1.0) — Relevance decay rate applied periodically (default: 0.95)
- **Min Relevance** (InputNumber, 0.0–1.0) — Minimum relevance score before a memory is pruned (default: 0.1)

#### Live Source Retrieval
When the knowledge base has no relevant documents, automatically fetch content from active MCP connectors (OneDrive, web pages, Slack, etc.) in real time. Requires at least one active connector in the workspace.
- **Enabled** (Switch) — Turn on/off live source retrieval
- **Timeout** (InputNumber, seconds) — Overall timeout for the retrieval stage (default: 15s)
- **Max Connectors** (InputNumber) — Maximum connectors to query in parallel (default: 3)
- **Max Content** (InputNumber, chars) — Maximum total characters to fetch across all connectors (default: 30,000)
- **LLM Override** — Optional LLM for connector selection (only used when more connectors are available than max)

### Prompts Tab
Manage system prompts:
- View all prompt templates
- Edit prompt overrides
- Delete custom overrides (reverts to default)

### Ollama Management Tab
For Ollama LLM provider:
- List downloaded models
- Pull new models

### MCP Connectors (API only)

MCP connector management is currently available via the API only. The Admin UI integration is planned for a future release. Super admins can manage connectors through the API endpoints:

- Create connectors from 9 built-in templates (GitHub, Confluence, Notion, Slack, Google Drive, PostgreSQL, SQLite, filesystem, web fetch)
- Trigger manual or scheduled (cron) syncs
- Monitor sync history and status
- Configure webhook notifications

See [API Reference](API_REFERENCE.md#mcp-connectors-super-admin) for endpoint details.

### Local Auth Tab
- Shows whether local authentication is enabled
- Configuration note: "Configure via `THAIRAG__AUTH__ENABLED` env var"

---

## Health

**Path:** `/system` | **Min role:** `viewer`

System health monitoring.

### Health Check
- Calls `GET /health?deep=true` to probe all providers
- Shows status for each subsystem:
  - API server
  - Database connection
  - LLM provider
  - Embedding provider
  - Vector database
  - Search engine (Tantivy)
  - Reranker
- Each shows green (healthy) or red (error with message)

### System Info
- Server version
- Uptime
- Configuration tier
- Provider details

---

## Config Snapshots

**Location:** Collapsible panel at the top of the Settings page (above the tabs)

Config Snapshots let super admins save and restore the entire system configuration as named restore points. This is useful before making major changes (e.g., switching LLM providers, changing embedding models, or adjusting pipeline settings).

### Saving a Snapshot
- Click **"Save Current Config"** in the snapshots panel header
- A modal prompts for:
  - **Name** (required, max 100 characters) — e.g., "Before switching to Claude"
  - **Description** (optional, max 500 characters) — notes about what this configuration represents
- The snapshot captures the full configuration including provider settings, pipeline config, presets, document processing settings, and all other configuration keys

### Snapshot Table
Each saved snapshot displays:
- **Name** — Snapshot name with description shown below in smaller text
- **Created** — Date and time the snapshot was taken
- **Embedding** — Embedding fingerprint badge (identifies the embedding model + dimension used when the snapshot was created)
- **Settings** — Number of configuration keys stored (shown as a tag, e.g., "42 keys")

### Restoring a Snapshot
- Click **Restore** on any snapshot row
- If the snapshot's embedding fingerprint matches the current configuration, the restore proceeds immediately
- If the embedding fingerprint differs, a confirmation dialog appears with two options:
  - **"Restore Without Embedding (Safe)"** — Restores all settings except the embedding configuration, avoiding a reindex
  - **"Restore Everything (Re-index Required)"** — Restores the full configuration including embedding settings; existing vector data will need to be re-indexed
- After restoring, reload the page to see the updated settings

### Deleting a Snapshot
- Click the delete button (trash icon) next to any snapshot
- A popconfirm dialog asks for confirmation before deletion

---

## Pipeline Stages UI

**Location:** Test Chat page — visible during and after query execution

The pipeline stages UI provides real-time visibility into what the RAG pipeline is doing as it processes a query.

### Live Progress Card
During query execution, a progress card appears showing each pipeline stage as it runs:
- **Spinning indicator** — Active stage currently being processed, shown with a highlighted background
- **Green checkmark** — Stage completed successfully
- **Minus icon** — Stage was skipped (e.g., an optional agent that wasn't needed)
- **Exclamation icon** — Stage encountered an error

Each stage row shows:
- **Friendly name** — Human-readable agent names instead of internal identifiers (e.g., "Query Analyzer" instead of `query_analyzer`, "Hybrid Search" instead of `search`, "Response Generator" instead of `response_generator`)
- **Task description** — Shown for the active stage in italic (e.g., "Analyzing intent, language & complexity", "Searching vector store & BM25 index")
- **Model tag** — The LLM model used by that stage, when applicable
- **Duration** — Shown for completed stages (milliseconds or seconds)

### Pipeline Stages Summary
After a response is received, a collapsible **"Pipeline Stages"** panel appears below the response:
- Header shows the total number of stages and total pipeline time (e.g., "Pipeline Stages (8) — 3,240ms total")
- If any stage took more than 1 second, a **Bottleneck** tag highlights the slowest stage
- Expanding the panel shows all stages with their status, model, and individual durations
- Slow stages (>2s) are highlighted with a warning background; very slow stages (>5s) get an error background

### Supported Stages
The UI maps the following internal stage names to friendly labels:

| Internal Name | Display Name | Task Description |
|---------------|-------------|------------------|
| `query_analyzer` | Query Analyzer | Analyzing intent, language & complexity |
| `self_rag_gate` | Self-RAG Gate | Deciding whether retrieval is needed |
| `pipeline_orchestrator` | Pipeline Orchestrator | Choosing the optimal pipeline route |
| `query_rewriter` | Query Rewriter | Rewriting query for better retrieval |
| `search` | Hybrid Search | Searching vector store & BM25 index |
| `colbert_reranker` | ColBERT Reranker | Re-ranking results with ColBERT |
| `graph_rag` | Graph RAG | Extracting entities & traversing knowledge graph |
| `context_curator` | Context Curator | Scoring & selecting the best context |
| `retrieval_refinement` | Retrieval Refinement | Refining retrieval with feedback signals |
| `corrective_rag` | Corrective RAG | Checking & correcting retrieved context |
| `live_retrieval` | Live Source Retrieval | Fetching from external sources (OneDrive, web, etc.) |
| `raptor` | RAPTOR | Building hierarchical document summaries |
| `contextual_compression` | Contextual Compression | Compressing context to key information |
| `multimodal_rag` | Multi-modal RAG | Processing images & tables from documents |
| `map_reduce` | Map-Reduce | Summarizing chunks in parallel |
| `response_generator` | Response Generator | Generating the final answer |
| `quality_guard` | Quality Guard | Checking answer quality & hallucinations |
| `language_adapter` | Language Adapter | Adapting response language to match query |

Any unrecognized stage names are auto-formatted by replacing underscores with spaces and capitalizing each word.

---

## Chat Persistence

**Location:** Test Chat page

Chat history and workspace selection are preserved across page navigation within the same browser tab using `sessionStorage`.

### What is Preserved
- **Messages** — All chat messages (user queries and assistant responses), including metadata such as retrieved chunks, usage stats, timing, pipeline stages, and feedback state
- **Workspace selection** — The selected organization, department, and workspace dropdowns

### Storage Behavior
- Uses `sessionStorage` (per-tab), so each browser tab has its own independent chat history
- Data is automatically cleared when the tab is closed
- If storage quota is exceeded, the write is silently ignored (chat continues to work but won't persist)

### Clearing Chat
- Click the **Clear Chat** button (broom icon) next to the Send button to manually reset the conversation
- Changing the workspace selection also clears the chat (since the context changes)

---

## Collapsible Settings

All settings sections across the Admin UI use collapsible panels (Ant Design `Collapse` components) to reduce visual clutter and let admins focus on the section they are configuring.

### Presets Tab
- **Chat Pipeline** — Collapsible group containing chat pipeline preset options
- **Document Processing** — Collapsible group containing document processing preset options

### Providers Tab
- **LLM Provider** — Collapsible card for LLM configuration (provider type, model, API key)
- **Reranker** — Collapsible card for reranker configuration

### Document Processing Tab
- **Pipeline Settings** — Collapsible section for chunk size, overlap, and upload limits
- **AI Preprocessing** — Collapsible section for AI-powered document preprocessing options
- **Embedding Config** — Collapsible section for embedding provider and dimension settings

### Other Tabs
All remaining settings tabs (Identity Providers, Chat Pipeline, Prompts, Vector DB, Ollama Management, Local Auth) wrap their content in collapsible sections following the same pattern.

Sections remember their expanded/collapsed state within the current page session.

---

## Theme

The Admin UI supports light and dark modes. Toggle via the sun/moon button in the top-right header. The preference persists in local storage.

## Navigation

- **Sidebar** — Collapsible sidebar with icons and labels for all pages
- **Header** — Shows logged-in user email, theme toggle, and logout button
- **Title** — Shows "ThaiRAG Admin" (or "TR" when collapsed)
