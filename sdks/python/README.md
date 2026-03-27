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

## Knowledge Management

```python
# List organizations
orgs = client.list_orgs()

# Create org → dept → workspace
org = client.create_org("My Org")
dept = client.create_dept(org["id"], "Engineering")
ws = client.create_workspace(org["id"], dept["id"], "Docs")

# Upload a document
doc = client.upload_document(ws["id"], "/path/to/file.pdf")

# Search
results = client.search(ws["id"], "deployment guide")
```

## Context Manager

```python
with ThaiRAGClient("http://localhost:8080", api_key="trag_...") as client:
    response = client.chat([{"role": "user", "content": "Hello"}])
```
