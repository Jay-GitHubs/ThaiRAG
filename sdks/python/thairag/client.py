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

    # ── Context manager ────────────────────────────────────────────────

    def close(self) -> None:
        """Close the underlying HTTP session."""
        self.session.close()

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()
