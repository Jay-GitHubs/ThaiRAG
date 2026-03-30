# ThaiRAG TypeScript Client

TypeScript/JavaScript client for the ThaiRAG API. Fully typed, zero dependencies,
supports both non-streaming and streaming (SSE) chat completions.

## Installation

```bash
npm install @thairag/client
```

## Quick Start

```typescript
import { ThaiRAGClient } from "@thairag/client";

const client = new ThaiRAGClient({
  baseUrl: "http://localhost:8080",
  apiKey: "trag_...",
});

const response = await client.chat([
  { role: "user", content: "What is ThaiRAG?" },
]);
console.log(response.choices[0].message.content);
```

## Authentication

The client supports two authentication methods: API key and JWT token.

```typescript
// Option 1: API key (passed as header X-API-Key)
const client = new ThaiRAGClient({
  baseUrl: "http://localhost:8080",
  apiKey: "trag_...",
});

// Option 2: Bearer token (if you already have a JWT)
const client = new ThaiRAGClient({
  baseUrl: "http://localhost:8080",
  token: "eyJhbGciOiJIUzI1NiIs...",
});

// Option 3: Login to obtain a token
const client = new ThaiRAGClient({ baseUrl: "http://localhost:8080" });
const token = await client.login("admin@example.com", "P@ssw0rd123");
// The client stores the token automatically for subsequent requests.
```

### Register a new user

```typescript
await client.register("user@example.com", "Jane Doe", "P@ssw0rd123");
```

## Chat Completions

### Non-streaming

```typescript
const response = await client.chat(
  [
    { role: "system", content: "You are a helpful assistant." },
    { role: "user", content: "Summarize our Q4 report." },
  ],
  { temperature: 0.3, max_tokens: 512 }
);

console.log(response.choices[0].message.content);
console.log(response.usage); // { prompt_tokens, completion_tokens, total_tokens }
```

### Streaming (SSE)

`chatStream` returns an `AsyncGenerator` that yields parsed SSE chunks as they
arrive. Each chunk follows the same `ChatResponse` shape, with partial content
available in `choices[0].delta.content`.

```typescript
const stream = client.chatStream(
  [{ role: "user", content: "Explain RAG in detail." }],
  { temperature: 0.7 }
);

for await (const chunk of stream) {
  const delta = chunk.choices[0]?.delta;
  if (delta?.content) {
    process.stdout.write(delta.content);
  }
}
```

## Knowledge Management

ThaiRAG organizes knowledge in a hierarchy: Organization > Department > Workspace > Documents.

### Organizations

```typescript
const orgs = await client.listOrgs();

const org = await client.createOrg("Acme Corp");
```

### Departments

```typescript
const depts = await client.listDepts(org.id);

const dept = await client.createDept(org.id, "Engineering");
```

### Workspaces

```typescript
const workspaces = await client.listWorkspaces(org.id, dept.id);

const ws = await client.createWorkspace(org.id, dept.id, "Backend Docs");
```

### Documents

```typescript
// Upload a document
const doc = await client.uploadDocument(
  ws.id,
  "ThaiRAG supports hybrid search combining vector and BM25...",
  "Architecture Overview",
  "text/plain"
);

// List documents in a workspace
const docs = await client.listDocuments(ws.id);

// Delete a document
await client.deleteDocument(ws.id, doc.id);
```

## Search

Run a hybrid search (vector + BM25 with RRF merge) against a workspace.

```typescript
const result = await client.search(ws.id, "deployment guide");

for (const hit of result.results) {
  console.log(`[${hit.score.toFixed(3)}] ${hit.title}`);
  console.log(hit.content);
}
```

## Feedback

Submit quality feedback on a chat response.

```typescript
await client.submitFeedback(response.id, 5, "Very helpful answer");
```

## Search Analytics

### Popular queries

```typescript
const popular = await client.getSearchAnalyticsPopular({
  fromDate: "2026-01-01",
  toDate: "2026-03-31",
  limit: 10,
});

for (const entry of popular) {
  console.log(`${entry.query} - ${entry.count} times`);
}
```

### Summary statistics

```typescript
const summary = await client.getSearchAnalyticsSummary({
  fromDate: "2026-01-01",
});

console.log(`Total queries: ${summary.total_queries}`);
console.log(`Avg latency: ${summary.avg_latency_ms}ms`);
console.log(`Zero-result rate: ${(summary.zero_result_rate * 100).toFixed(1)}%`);
```

## Document Lineage

Track which document chunks contributed to a given response, or find all
responses that used a particular document.

### By response

```typescript
const records = await client.getLineageByResponse(response.id);

for (const r of records) {
  console.log(`doc=${r.doc_id} chunk=${r.chunk_id} score=${r.score} rank=${r.rank}`);
}
```

### By document

```typescript
const records = await client.getLineageByDocument(doc.id);
```

## Audit Log

### Export entries

```typescript
const entries = await client.exportAuditLog({
  format: "json",
  fromDate: "2026-03-01",
  action: "login",
});

for (const entry of entries) {
  console.log(`[${entry.timestamp}] ${entry.action}: ${entry.detail} (${entry.success ? "ok" : "fail"})`);
}
```

### Analytics

```typescript
const analytics = await client.getAuditAnalytics({
  fromDate: "2026-03-01",
});

console.log(`Total events: ${analytics.total_events}`);
console.log(`Success rate: ${analytics.success_rate}`);

for (const [action, count] of analytics.actions_by_type) {
  console.log(`  ${action}: ${count}`);
}
```

## Multi-Tenancy

Manage tenants, their plans, and lifecycle.

```typescript
// List all tenants
const tenants = await client.listTenants();

// Create a tenant
const tenant = await client.createTenant("Customer A", "standard");
console.log(`Tenant ${tenant.id} on plan "${tenant.plan}"`);

// Delete a tenant
await client.deleteTenant(tenant.id);
```

## Custom Roles

Define roles with fine-grained resource permissions.

```typescript
// List existing roles
const roles = await client.listRoles();

// Create a role with permissions
const role = await client.createRole("doc-manager", "Can manage documents", [
  { resource: "documents", actions: ["read", "write", "delete"] },
  { resource: "workspaces", actions: ["read"] },
]);

// Delete a role
await client.deleteRole(role.id);
```

## Prompt Marketplace

Browse, create, and share prompt templates.

```typescript
// List prompts, optionally filtered
const prompts = await client.listPrompts({ category: "summarization" });
const searchResults = await client.listPrompts({ search: "translate" });

// Create a prompt template
const prompt = await client.createPrompt(
  "Summarize Document",
  "Summarize the following text in {{language}}:\n\n{{text}}",
  "summarization",
  ["language", "text"]
);

// Delete a prompt
await client.deletePrompt(prompt.id);
```

## Embedding Fine-Tuning

Manage datasets and fine-tuning jobs for embedding models.

### Datasets

```typescript
const datasets = await client.listFinetuneDatasets();

const dataset = await client.createFinetuneDataset(
  "Thai Legal Pairs",
  "Query-document pairs from Thai legal corpus"
);
```

### Jobs

```typescript
const jobs = await client.listFinetuneJobs();

for (const job of jobs) {
  console.log(`Job ${job.id}: dataset=${job.dataset_id} status=${job.status}`);
}
```

## Personal Memory

Access and manage per-user memory entries stored by the system.

```typescript
const memories = await client.listMemories(userId);

// Delete a specific memory
await client.deleteMemory(userId, memories[0].id as string);
```

## Error Handling

All methods throw an `Error` with the HTTP status code and response body when a
request fails.

```typescript
try {
  await client.chat([{ role: "user", content: "Hello" }]);
} catch (err) {
  if (err instanceof Error) {
    // err.message is formatted as "STATUS: BODY"
    // e.g. "401: Unauthorized" or "422: {\"error\":\"invalid model\"}"
    console.error("API error:", err.message);
  }
}
```

## Configuration

| Option    | Default                 | Description                         |
|-----------|-------------------------|-------------------------------------|
| `baseUrl` | `http://localhost:8080` | ThaiRAG server URL                  |
| `apiKey`  | -                       | API key sent as `X-API-Key` header  |
| `token`   | -                       | Bearer token sent in `Authorization`|

All options are passed to the `ThaiRAGClient` constructor:

```typescript
const client = new ThaiRAGClient({
  baseUrl: "https://thairag.example.com",
  apiKey: "trag_production_key",
});
```

## License

AGPL-3.0
