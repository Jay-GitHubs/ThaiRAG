# ThaiRAG Python Client

## Install

```
pip install thairag
```

## Usage

```python
from thairag import ThaiRAGClient

client = ThaiRAGClient("http://localhost:8080", api_key="trag_...")
response = client.chat([{"role": "user", "content": "Hello"}])
print(response["choices"][0]["message"]["content"])
```

## Authentication

```python
# API key
client = ThaiRAGClient("http://localhost:8080", api_key="trag_...")

# JWT (login)
client = ThaiRAGClient("http://localhost:8080")
token = client.login("admin@example.com", "password")
```

## Streaming

```python
for chunk in client.chat([{"role": "user", "content": "Hello"}], stream=True):
    delta = chunk["choices"][0].get("delta", {})
    print(delta.get("content", ""), end="", flush=True)
```

The streaming endpoint uses Server-Sent Events (SSE). Each chunk is a parsed
dict matching the OpenAI streaming format. A final usage chunk is emitted
before the stream closes, containing token counts:

```python
for chunk in client.chat([{"role": "user", "content": "Summarize this"}], stream=True):
    choices = chunk.get("choices", [])
    if choices:
        delta = choices[0].get("delta", {})
        content = delta.get("content", "")
        if content:
            print(content, end="", flush=True)

    # The last chunk before [DONE] contains usage stats
    usage = chunk.get("usage")
    if usage:
        print(f"\nTokens: {usage['prompt_tokens']} in, {usage['completion_tokens']} out")
```

## Knowledge Management

```python
# List organizations
orgs = client.list_orgs()

# Create org -> dept -> workspace
org = client.create_org("My Org")
dept = client.create_dept(org["id"], "Engineering")
ws = client.create_workspace(org["id"], dept["id"], "Docs")

# Upload a document
doc = client.upload_document(ws["id"], "/path/to/file.pdf")

# Search
results = client.search(ws["id"], "deployment guide")
```

## Context Manager

The client wraps an `httpx.Client` session internally. Use it as a context
manager to ensure the HTTP connection pool is closed cleanly:

```python
with ThaiRAGClient("http://localhost:8080", api_key="trag_...") as client:
    response = client.chat([{"role": "user", "content": "Hello"}])
    print(response["choices"][0]["message"]["content"])
# session is automatically closed here
```

You can also close it manually:

```python
client = ThaiRAGClient("http://localhost:8080", api_key="trag_...")
try:
    response = client.chat([{"role": "user", "content": "Hello"}])
finally:
    client.close()
```

## Error Handling

All methods raise `httpx.HTTPStatusError` on non-2xx responses. You can catch
these to handle specific error codes:

```python
import httpx

client = ThaiRAGClient("http://localhost:8080", api_key="trag_...")

try:
    response = client.chat([{"role": "user", "content": "Hello"}])
except httpx.HTTPStatusError as e:
    if e.response.status_code == 401:
        print("Authentication failed -- check your API key")
    elif e.response.status_code == 429:
        print("Rate limited -- back off and retry")
    elif e.response.status_code == 502:
        print("Upstream LLM provider is unavailable")
    else:
        print(f"Request failed: {e.response.status_code} {e.response.text}")
except httpx.ConnectError:
    print("Could not connect to ThaiRAG server")
```

For operations that return `None` on success (like delete methods), the
absence of an exception indicates success:

```python
try:
    client.delete_document(ws_id, doc_id)
    print("Document deleted")
except httpx.HTTPStatusError as e:
    print(f"Delete failed: {e.response.status_code}")
```

## Search Analytics

Track search usage patterns and surface popular queries. Useful for
understanding what users are looking for and identifying content gaps.

```python
# Get the most popular search queries in a date range
popular = client.get_search_analytics_popular(
    from_date="2026-01-01",
    to_date="2026-03-31",
    limit=10,
)
for entry in popular:
    print(f"{entry['query']}: {entry['count']} searches")

# Get an aggregate summary of search activity
summary = client.get_search_analytics_summary(
    from_date="2026-01-01",
    to_date="2026-03-31",
)
print(f"Total searches: {summary['total_searches']}")
print(f"Unique queries: {summary['unique_queries']}")
```

Both methods accept optional `from_date` and `to_date` parameters as
ISO 8601 date strings. When omitted, the server returns data for the
default time window.

## Document Lineage

Trace which source documents contributed to a generated response, or find
all responses that referenced a particular document.

```python
# After a chat response, trace its sources
response = client.chat([{"role": "user", "content": "What is our refund policy?"}])
response_id = response["id"]

lineage = client.get_lineage_by_response(response_id)
for record in lineage:
    print(f"Document: {record['document_id']}, chunk: {record['chunk_id']}")
    print(f"  Relevance score: {record.get('score', 'N/A')}")

# Find every response that cited a specific document
doc_lineage = client.get_lineage_by_document("doc_abc123")
for record in doc_lineage:
    print(f"Response {record['response_id']} used this document")
```

## Audit Log

Export and analyze the audit trail of all administrative actions. Requires
admin privileges.

```python
# Export the full audit log as JSON
entries = client.export_audit_log(
    format="json",
    from_date="2026-03-01",
    to_date="2026-03-31",
    action="document.upload",
)
for entry in entries:
    print(f"{entry['timestamp']} | {entry['action']} by {entry['user_id']}")

# Export as CSV (returns raw CSV text depending on server implementation)
csv_data = client.export_audit_log(format="csv")

# Get analytics over the audit log (action counts, active users, etc.)
analytics = client.get_audit_analytics(
    from_date="2026-03-01",
    to_date="2026-03-31",
)
print(f"Total events: {analytics['total_events']}")
for action, count in analytics.get("by_action", {}).items():
    print(f"  {action}: {count}")
```

## Multi-Tenancy

Manage isolated tenants, each with their own data and configuration.
Requires super-admin privileges.

```python
# List all tenants
tenants = client.list_tenants()
for t in tenants:
    print(f"{t['id']}: {t['name']} (plan: {t['plan']})")

# Create a new tenant
tenant = client.create_tenant("Acme Corp", plan="standard")
print(f"Created tenant: {tenant['id']}")

# Delete a tenant (irreversible -- removes all tenant data)
client.delete_tenant(tenant["id"])
```

## Custom Roles

Define fine-grained access control roles with specific permission sets.

```python
# List existing roles
roles = client.list_roles()
for role in roles:
    print(f"{role['name']}: {role.get('description', '')}")
    print(f"  Permissions: {role.get('permissions', [])}")

# Create a role with specific permissions
role = client.create_role(
    name="doc-reviewer",
    description="Can read documents and submit feedback",
    permissions=["document.read", "feedback.submit"],
)
print(f"Created role: {role['id']}")

# Delete a custom role
client.delete_role(role["id"])
```

## Prompt Marketplace

Browse, create, and manage reusable prompt templates.

```python
# List all available prompts
prompts = client.list_prompts()
for p in prompts:
    print(f"{p['name']} [{p['category']}]")

# Filter by category or search term
legal_prompts = client.list_prompts(category="legal")
results = client.list_prompts(search="summarize")

# Create a prompt template with variable placeholders
prompt = client.create_prompt(
    name="Contract Summarizer",
    content="Summarize the following contract in {{language}}: {{contract_text}}",
    category="legal",
    variables=["language", "contract_text"],
)
print(f"Created prompt: {prompt['id']}")

# Delete a prompt template
client.delete_prompt(prompt["id"])
```

## Embedding Fine-Tuning

Create domain-specific datasets and launch fine-tuning jobs to improve
embedding quality for your use case.

```python
# List existing fine-tuning datasets
datasets = client.list_finetune_datasets()
for ds in datasets:
    print(f"{ds['id']}: {ds['name']} -- {ds.get('description', '')}")

# Create a new dataset
dataset = client.create_finetune_dataset(
    name="thai-legal-pairs",
    description="Query-document pairs from Thai legal corpus",
)
print(f"Created dataset: {dataset['id']}")

# List fine-tuning jobs and check their status
jobs = client.list_finetune_jobs()
for job in jobs:
    print(f"Job {job['id']}: status={job['status']}, dataset={job['dataset_id']}")
```

## Personal Memory

Per-user memory allows the system to remember user preferences and context
across sessions. Memories are created automatically during chat; use these
methods to inspect or remove them.

```python
user_id = "user_abc123"

# List all stored memories for a user
memories = client.list_memories(user_id)
for mem in memories:
    print(f"{mem['id']}: {mem['content']}")

# Delete a specific memory
client.delete_memory(user_id, memories[0]["id"])
```

## Full Example

A complete workflow that ties multiple features together:

```python
from thairag import ThaiRAGClient

with ThaiRAGClient("http://localhost:8080", api_key="trag_...") as client:
    # Set up knowledge base
    org = client.create_org("Demo Org")
    dept = client.create_dept(org["id"], "Support")
    ws = client.create_workspace(org["id"], dept["id"], "FAQ")

    # Upload content
    client.upload_document(ws["id"], "faq.pdf")

    # Ask a question (streaming)
    print("Answer: ", end="")
    for chunk in client.chat(
        [{"role": "user", "content": "How do I reset my password?"}],
        stream=True,
    ):
        choices = chunk.get("choices", [])
        if choices:
            content = choices[0].get("delta", {}).get("content", "")
            print(content, end="", flush=True)
    print()

    # Check what people are searching for
    popular = client.get_search_analytics_popular(limit=5)
    print("Top queries:", [q["query"] for q in popular])
```
