# ThaiRAG API Guide

## Overview

ThaiRAG exposes an OpenAI-compatible API under a single model identity: **ThaiRAG-1.0**.

## Endpoints

### Health Check
```
GET /health
```

### List Models
```
GET /v1/models
```

### Chat Completions
```
POST /v1/chat/completions
Content-Type: application/json

{
  "model": "ThaiRAG-1.0",
  "messages": [
    {"role": "user", "content": "Your question here"}
  ]
}
```

## Configuration

See `config/default.toml` for all available settings. Override with:
- `config/local.toml` for local development
- `THAIRAG_TIER` env var to select a tier preset (free/standard/premium)
- `THAIRAG__*` env vars for individual settings
