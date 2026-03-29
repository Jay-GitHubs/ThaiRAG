import httpx
from typing import Optional, List, Iterator


class ThaiRAGClient:
    """Python client for the ThaiRAG API."""

    def __init__(
        self,
        base_url: str = "http://localhost:8080",
        api_key: Optional[str] = None,
        token: Optional[str] = None,
    ):
        self.base_url = base_url.rstrip("/")
        self.session = httpx.Client(timeout=60)
        self.headers: dict = {}
        if api_key:
            self.headers["X-API-Key"] = api_key
        if token:
            self.headers["Authorization"] = f"Bearer {token}"

    # ── Authentication ─────────────────────────────────────────────────

    def login(self, email: str, password: str) -> str:
        """Login and store JWT token. Returns the token."""
        resp = self.session.post(
            f"{self.base_url}/api/auth/login",
            json={"email": email, "password": password},
        )
        resp.raise_for_status()
        data = resp.json()
        token = data.get("token") or data.get("access_token", "")
        self.headers["Authorization"] = f"Bearer {token}"
        return token

    def register(self, email: str, name: str, password: str) -> dict:
        """Register a new user."""
        resp = self.session.post(
            f"{self.base_url}/api/auth/register",
            json={"email": email, "name": name, "password": password},
        )
        resp.raise_for_status()
        return resp.json()

    # ── Health / Models ────────────────────────────────────────────────

    def health(self, deep: bool = False) -> dict:
        """Check system health."""
        url = f"{self.base_url}/health"
        if deep:
            url += "?deep=true"
        resp = self.session.get(url, headers=self.headers)
        resp.raise_for_status()
        return resp.json()

    def list_models(self) -> dict:
        """List available models (OpenAI-compatible)."""
        resp = self.session.get(
            f"{self.base_url}/v1/models", headers=self.headers
        )
        resp.raise_for_status()
        return resp.json()

    # ── Chat ───────────────────────────────────────────────────────────

    def chat(
        self,
        messages: List[dict],
        model: str = "ThaiRAG-1.0",
        stream: bool = False,
        **kwargs,
    ):
        """Send a chat completion request.

        If stream=True, yields parsed SSE chunks as dicts.
        Otherwise returns the full response dict.
        """
        body = {"model": model, "messages": messages, "stream": stream, **kwargs}
        if stream:
            return self._chat_stream(body)
        resp = self.session.post(
            f"{self.base_url}/v1/chat/completions",
            headers={**self.headers, "Content-Type": "application/json"},
            json=body,
        )
        resp.raise_for_status()
        return resp.json()

    def _chat_stream(self, body: dict) -> Iterator[dict]:
        """Internal generator that yields SSE chunks as dicts."""
        import json

        with self.session.stream(
            "POST",
            f"{self.base_url}/v1/chat/completions",
            headers={**self.headers, "Content-Type": "application/json"},
            json=body,
        ) as response:
            response.raise_for_status()
            for line in response.iter_lines():
                if line.startswith("data: "):
                    payload = line[6:].strip()
                    if payload == "[DONE]":
                        break
                    try:
                        yield json.loads(payload)
                    except json.JSONDecodeError:
                        pass

    def chat_v2(self, messages: List[dict], **kwargs) -> dict:
        """V2 chat with metadata and sources."""
        resp = self.session.post(
            f"{self.base_url}/v2/chat/completions",
            headers={**self.headers, "Content-Type": "application/json"},
            json={"messages": messages, **kwargs},
        )
        resp.raise_for_status()
        return resp.json()

    # ── Organizations ──────────────────────────────────────────────────

    def list_orgs(self) -> List[dict]:
        """List all organizations."""
        resp = self.session.get(
            f"{self.base_url}/api/km/orgs", headers=self.headers
        )
        resp.raise_for_status()
        return resp.json()

    def create_org(self, name: str) -> dict:
        """Create a new organization."""
        resp = self.session.post(
            f"{self.base_url}/api/km/orgs",
            headers={**self.headers, "Content-Type": "application/json"},
            json={"name": name},
        )
        resp.raise_for_status()
        return resp.json()

    # ── Departments ────────────────────────────────────────────────────

    def list_depts(self, org_id: str) -> List[dict]:
        """List departments for an organization."""
        resp = self.session.get(
            f"{self.base_url}/api/km/orgs/{org_id}/depts",
            headers=self.headers,
        )
        resp.raise_for_status()
        return resp.json()

    def create_dept(self, org_id: str, name: str) -> dict:
        """Create a new department."""
        resp = self.session.post(
            f"{self.base_url}/api/km/orgs/{org_id}/depts",
            headers={**self.headers, "Content-Type": "application/json"},
            json={"name": name},
        )
        resp.raise_for_status()
        return resp.json()

    # ── Workspaces ─────────────────────────────────────────────────────

    def list_workspaces(self, org_id: str, dept_id: str) -> List[dict]:
        """List workspaces for a department."""
        resp = self.session.get(
            f"{self.base_url}/api/km/orgs/{org_id}/depts/{dept_id}/workspaces",
            headers=self.headers,
        )
        resp.raise_for_status()
        return resp.json()

    def create_workspace(self, org_id: str, dept_id: str, name: str) -> dict:
        """Create a new workspace."""
        resp = self.session.post(
            f"{self.base_url}/api/km/orgs/{org_id}/depts/{dept_id}/workspaces",
            headers={**self.headers, "Content-Type": "application/json"},
            json={"name": name},
        )
        resp.raise_for_status()
        return resp.json()

    # ── Documents ──────────────────────────────────────────────────────

    def upload_document(self, workspace_id: str, file_path: str) -> dict:
        """Upload a file to a workspace."""
        with open(file_path, "rb") as f:
            file_content = f.read()
        import os
        filename = os.path.basename(file_path)
        resp = self.session.post(
            f"{self.base_url}/api/km/workspaces/{workspace_id}/documents/upload",
            headers=self.headers,
            files={"file": (filename, file_content)},
        )
        resp.raise_for_status()
        return resp.json()

    def list_documents(self, workspace_id: str) -> List[dict]:
        """List documents in a workspace."""
        resp = self.session.get(
            f"{self.base_url}/api/km/workspaces/{workspace_id}/documents",
            headers=self.headers,
        )
        resp.raise_for_status()
        return resp.json()

    def delete_document(self, workspace_id: str, doc_id: str) -> None:
        """Delete a document from a workspace."""
        resp = self.session.delete(
            f"{self.base_url}/api/km/workspaces/{workspace_id}/documents/{doc_id}",
            headers=self.headers,
        )
        resp.raise_for_status()

    # ── Search ─────────────────────────────────────────────────────────

    def search(self, workspace_id: str, query: str) -> dict:
        """Search documents in a workspace."""
        resp = self.session.post(
            f"{self.base_url}/api/km/workspaces/{workspace_id}/test-query",
            headers={**self.headers, "Content-Type": "application/json"},
            json={"query": query},
        )
        resp.raise_for_status()
        return resp.json()

    # ── Feedback ───────────────────────────────────────────────────────

    def submit_feedback(
        self, response_id: str, rating: int, comment: str = ""
    ) -> dict:
        """Submit feedback for a chat response."""
        resp = self.session.post(
            f"{self.base_url}/v1/chat/feedback",
            headers={**self.headers, "Content-Type": "application/json"},
            json={"response_id": response_id, "rating": rating, "comment": comment},
        )
        resp.raise_for_status()
        return resp.json()

    # ── Search Analytics ───────────────────────────────────────────────

    def get_search_analytics_popular(
        self, from_date=None, to_date=None, limit=20
    ) -> list:
        """Get popular search queries."""
        params = {"limit": limit}
        if from_date:
            params["from"] = from_date
        if to_date:
            params["to"] = to_date
        resp = self.session.get(
            f"{self.base_url}/api/km/search-analytics/popular",
            headers=self.headers,
            params=params,
        )
        resp.raise_for_status()
        return resp.json()

    def get_search_analytics_summary(self, from_date=None, to_date=None) -> dict:
        """Get search analytics summary."""
        params = {}
        if from_date:
            params["from"] = from_date
        if to_date:
            params["to"] = to_date
        resp = self.session.get(
            f"{self.base_url}/api/km/search-analytics/summary",
            headers=self.headers,
            params=params,
        )
        resp.raise_for_status()
        return resp.json()

    # ── Lineage ────────────────────────────────────────────────────────

    def get_lineage_by_response(self, response_id: str) -> list:
        """Get lineage records for a response."""
        resp = self.session.get(
            f"{self.base_url}/api/km/lineage/response/{response_id}",
            headers=self.headers,
        )
        resp.raise_for_status()
        return resp.json()

    def get_lineage_by_document(self, document_id: str) -> list:
        """Get lineage records for a document."""
        resp = self.session.get(
            f"{self.base_url}/api/km/lineage/document/{document_id}",
            headers=self.headers,
        )
        resp.raise_for_status()
        return resp.json()

    # ── Audit Log ──────────────────────────────────────────────────────

    def export_audit_log(
        self, format="json", from_date=None, to_date=None, action=None
    ):
        """Export the audit log."""
        params = {"format": format}
        if from_date:
            params["from"] = from_date
        if to_date:
            params["to"] = to_date
        if action:
            params["action"] = action
        resp = self.session.get(
            f"{self.base_url}/api/km/settings/audit-log/export",
            headers=self.headers,
            params=params,
        )
        resp.raise_for_status()
        return resp.json()

    def get_audit_analytics(self, from_date=None, to_date=None) -> dict:
        """Get audit log analytics."""
        params = {}
        if from_date:
            params["from"] = from_date
        if to_date:
            params["to"] = to_date
        resp = self.session.get(
            f"{self.base_url}/api/km/settings/audit-log/analytics",
            headers=self.headers,
            params=params,
        )
        resp.raise_for_status()
        return resp.json()

    # ── Tenants ────────────────────────────────────────────────────────

    def list_tenants(self) -> list:
        """List all tenants."""
        resp = self.session.get(
            f"{self.base_url}/api/km/tenants", headers=self.headers
        )
        resp.raise_for_status()
        return resp.json()

    def create_tenant(self, name: str, plan: str = "free") -> dict:
        """Create a new tenant."""
        resp = self.session.post(
            f"{self.base_url}/api/km/tenants",
            headers={**self.headers, "Content-Type": "application/json"},
            json={"name": name, "plan": plan},
        )
        resp.raise_for_status()
        return resp.json()

    def delete_tenant(self, tenant_id: str) -> None:
        """Delete a tenant."""
        resp = self.session.delete(
            f"{self.base_url}/api/km/tenants/{tenant_id}",
            headers=self.headers,
        )
        resp.raise_for_status()

    # ── Roles ──────────────────────────────────────────────────────────

    def list_roles(self) -> list:
        """List all custom roles."""
        resp = self.session.get(
            f"{self.base_url}/api/km/roles", headers=self.headers
        )
        resp.raise_for_status()
        return resp.json()

    def create_role(
        self, name: str, description: str = "", permissions=None
    ) -> dict:
        """Create a custom role."""
        resp = self.session.post(
            f"{self.base_url}/api/km/roles",
            headers={**self.headers, "Content-Type": "application/json"},
            json={
                "name": name,
                "description": description,
                "permissions": permissions or [],
            },
        )
        resp.raise_for_status()
        return resp.json()

    def delete_role(self, role_id: str) -> None:
        """Delete a custom role."""
        resp = self.session.delete(
            f"{self.base_url}/api/km/roles/{role_id}",
            headers=self.headers,
        )
        resp.raise_for_status()

    # ── Prompt Marketplace ─────────────────────────────────────────────

    def list_prompts(self, category=None, search=None) -> list:
        """List prompt templates in the marketplace."""
        params = {}
        if category:
            params["category"] = category
        if search:
            params["search"] = search
        resp = self.session.get(
            f"{self.base_url}/api/km/prompts/marketplace",
            headers=self.headers,
            params=params,
        )
        resp.raise_for_status()
        return resp.json()

    def create_prompt(
        self,
        name: str,
        content: str,
        category: str = "general",
        variables=None,
    ) -> dict:
        """Create a prompt template."""
        resp = self.session.post(
            f"{self.base_url}/api/km/prompts/marketplace",
            headers={**self.headers, "Content-Type": "application/json"},
            json={
                "name": name,
                "content": content,
                "category": category,
                "variables": variables or [],
            },
        )
        resp.raise_for_status()
        return resp.json()

    def delete_prompt(self, prompt_id: str) -> None:
        """Delete a prompt template."""
        resp = self.session.delete(
            f"{self.base_url}/api/km/prompts/marketplace/{prompt_id}",
            headers=self.headers,
        )
        resp.raise_for_status()

    # ── Fine-tuning ────────────────────────────────────────────────────

    def list_finetune_datasets(self) -> list:
        """List fine-tuning datasets."""
        resp = self.session.get(
            f"{self.base_url}/api/km/finetune/datasets", headers=self.headers
        )
        resp.raise_for_status()
        return resp.json()

    def create_finetune_dataset(
        self, name: str, description: str = ""
    ) -> dict:
        """Create a fine-tuning dataset."""
        resp = self.session.post(
            f"{self.base_url}/api/km/finetune/datasets",
            headers={**self.headers, "Content-Type": "application/json"},
            json={"name": name, "description": description},
        )
        resp.raise_for_status()
        return resp.json()

    def list_finetune_jobs(self) -> list:
        """List fine-tuning jobs."""
        resp = self.session.get(
            f"{self.base_url}/api/km/finetune/jobs", headers=self.headers
        )
        resp.raise_for_status()
        return resp.json()

    # ── Personal Memory ────────────────────────────────────────────────

    def list_memories(self, user_id: str) -> list:
        """List personal memories for a user."""
        resp = self.session.get(
            f"{self.base_url}/api/km/users/{user_id}/memories",
            headers=self.headers,
        )
        resp.raise_for_status()
        return resp.json()

    def delete_memory(self, user_id: str, memory_id: str) -> None:
        """Delete a personal memory for a user."""
        resp = self.session.delete(
            f"{self.base_url}/api/km/users/{user_id}/memories/{memory_id}",
            headers=self.headers,
        )
        resp.raise_for_status()

    # ── Context manager ────────────────────────────────────────────────

    def close(self) -> None:
        """Close the underlying HTTP session."""
        self.session.close()

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()
