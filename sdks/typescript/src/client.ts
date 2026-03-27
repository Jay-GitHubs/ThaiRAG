import type {
  ChatMessage,
  ChatOptions,
  ChatResponse,
  Department,
  Document,
  FeedbackResponse,
  HealthResponse,
  ModelsResponse,
  Organization,
  SearchResult,
  Workspace,
} from "./types";

export interface ThaiRAGClientOptions {
  baseUrl?: string;
  apiKey?: string;
  token?: string;
}

export class ThaiRAGClient {
  private baseUrl: string;
  private headers: Record<string, string>;

  constructor(options: ThaiRAGClientOptions = {}) {
    this.baseUrl = (options.baseUrl || "http://localhost:8080").replace(
      /\/$/,
      ""
    );
    this.headers = { "Content-Type": "application/json" };
    if (options.apiKey) {
      this.headers["X-API-Key"] = options.apiKey;
    }
    if (options.token) {
      this.headers["Authorization"] = `Bearer ${options.token}`;
    }
  }

  // ── Authentication ─────────────────────────────────────────────────

  async login(email: string, password: string): Promise<string> {
    const data = await this.request<{ token?: string; access_token?: string }>(
      "POST",
      "/api/auth/login",
      { email, password }
    );
    const token = data.token || data.access_token || "";
    this.headers["Authorization"] = `Bearer ${token}`;
    return token;
  }

  async register(
    email: string,
    name: string,
    password: string
  ): Promise<Record<string, unknown>> {
    return this.request("POST", "/api/auth/register", {
      email,
      name,
      password,
    });
  }

  // ── Health / Models ────────────────────────────────────────────────

  async health(deep?: boolean): Promise<HealthResponse> {
    const path = deep ? "/health?deep=true" : "/health";
    return this.request<HealthResponse>("GET", path);
  }

  async listModels(): Promise<ModelsResponse> {
    return this.request<ModelsResponse>("GET", "/v1/models");
  }

  // ── Chat ───────────────────────────────────────────────────────────

  async chat(
    messages: ChatMessage[],
    options: ChatOptions & { stream?: false } = {}
  ): Promise<ChatResponse> {
    const { stream: _stream, ...rest } = options;
    const body = {
      model: options.model || "ThaiRAG-1.0",
      messages,
      stream: false,
      ...rest,
    };
    return this.request<ChatResponse>("POST", "/v1/chat/completions", body);
  }

  async *chatStream(
    messages: ChatMessage[],
    options: ChatOptions = {}
  ): AsyncGenerator<ChatResponse> {
    const body = {
      model: options.model || "ThaiRAG-1.0",
      messages,
      stream: true,
      ...options,
    };

    const res = await fetch(`${this.baseUrl}/v1/chat/completions`, {
      method: "POST",
      headers: this.headers,
      body: JSON.stringify(body),
    });

    if (!res.ok || !res.body) {
      throw new Error(`${res.status}: ${await res.text()}`);
    }

    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";

    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split("\n");
      buffer = lines.pop() ?? "";
      for (const line of lines) {
        if (line.startsWith("data: ")) {
          const payload = line.slice(6).trim();
          if (payload === "[DONE]") return;
          try {
            yield JSON.parse(payload) as ChatResponse;
          } catch {
            // skip malformed lines
          }
        }
      }
    }
  }

  async chatV2(
    messages: ChatMessage[],
    options: ChatOptions = {}
  ): Promise<Record<string, unknown>> {
    return this.request("POST", "/v2/chat/completions", {
      messages,
      ...options,
    });
  }

  // ── Organizations ──────────────────────────────────────────────────

  async listOrgs(): Promise<Organization[]> {
    return this.request<Organization[]>("GET", "/api/km/orgs");
  }

  async createOrg(name: string): Promise<Organization> {
    return this.request<Organization>("POST", "/api/km/orgs", { name });
  }

  // ── Departments ────────────────────────────────────────────────────

  async listDepts(orgId: string): Promise<Department[]> {
    return this.request<Department[]>(
      "GET",
      `/api/km/orgs/${orgId}/depts`
    );
  }

  async createDept(orgId: string, name: string): Promise<Department> {
    return this.request<Department>(
      "POST",
      `/api/km/orgs/${orgId}/depts`,
      { name }
    );
  }

  // ── Workspaces ─────────────────────────────────────────────────────

  async listWorkspaces(orgId: string, deptId: string): Promise<Workspace[]> {
    return this.request<Workspace[]>(
      "GET",
      `/api/km/orgs/${orgId}/depts/${deptId}/workspaces`
    );
  }

  async createWorkspace(
    orgId: string,
    deptId: string,
    name: string
  ): Promise<Workspace> {
    return this.request<Workspace>(
      "POST",
      `/api/km/orgs/${orgId}/depts/${deptId}/workspaces`,
      { name }
    );
  }

  // ── Documents ──────────────────────────────────────────────────────

  async uploadDocument(
    workspaceId: string,
    content: string,
    title: string,
    mimeType: string = "text/plain"
  ): Promise<Document> {
    return this.request<Document>(
      "POST",
      `/api/km/workspaces/${workspaceId}/documents`,
      { content, title, mime_type: mimeType }
    );
  }

  async listDocuments(workspaceId: string): Promise<Document[]> {
    return this.request<Document[]>(
      "GET",
      `/api/km/workspaces/${workspaceId}/documents`
    );
  }

  async deleteDocument(workspaceId: string, docId: string): Promise<void> {
    await this.request(
      "DELETE",
      `/api/km/workspaces/${workspaceId}/documents/${docId}`
    );
  }

  // ── Search ─────────────────────────────────────────────────────────

  async search(workspaceId: string, query: string): Promise<SearchResult> {
    return this.request<SearchResult>(
      "POST",
      `/api/km/workspaces/${workspaceId}/test-query`,
      { query }
    );
  }

  // ── Feedback ───────────────────────────────────────────────────────

  async submitFeedback(
    responseId: string,
    rating: number,
    comment: string = ""
  ): Promise<FeedbackResponse> {
    return this.request<FeedbackResponse>("POST", "/v1/chat/feedback", {
      response_id: responseId,
      rating,
      comment,
    });
  }

  // ── Internal helpers ───────────────────────────────────────────────

  private async request<T>(
    method: string,
    path: string,
    body?: unknown
  ): Promise<T> {
    const res = await fetch(`${this.baseUrl}${path}`, {
      method,
      headers: this.headers,
      body: body !== undefined ? JSON.stringify(body) : undefined,
    });
    if (!res.ok) {
      throw new Error(`${res.status}: ${await res.text()}`);
    }
    // Handle 204 No Content
    if (res.status === 204) {
      return undefined as unknown as T;
    }
    return res.json() as Promise<T>;
  }
}
