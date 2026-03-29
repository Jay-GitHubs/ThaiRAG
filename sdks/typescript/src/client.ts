import type {
  AuditAnalytics,
  AuditLogEntry,
  ChatMessage,
  ChatOptions,
  ChatResponse,
  CustomRole,
  Department,
  Document,
  FeedbackResponse,
  FinetuneDataset,
  FinetuneJob,
  HealthResponse,
  LineageRecord,
  ModelsResponse,
  Organization,
  Permission,
  PromptTemplate,
  SearchAnalyticsPopular,
  SearchAnalyticsSummary,
  SearchResult,
  Tenant,
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

  // ── Search Analytics ───────────────────────────────────────────────

  async getSearchAnalyticsPopular(opts: {
    fromDate?: string;
    toDate?: string;
    limit?: number;
  } = {}): Promise<SearchAnalyticsPopular[]> {
    const params = new URLSearchParams();
    params.set("limit", String(opts.limit ?? 20));
    if (opts.fromDate) params.set("from", opts.fromDate);
    if (opts.toDate) params.set("to", opts.toDate);
    return this.request<SearchAnalyticsPopular[]>(
      "GET",
      `/api/km/search-analytics/popular?${params}`
    );
  }

  async getSearchAnalyticsSummary(opts: {
    fromDate?: string;
    toDate?: string;
  } = {}): Promise<SearchAnalyticsSummary> {
    const params = new URLSearchParams();
    if (opts.fromDate) params.set("from", opts.fromDate);
    if (opts.toDate) params.set("to", opts.toDate);
    const qs = params.toString();
    return this.request<SearchAnalyticsSummary>(
      "GET",
      `/api/km/search-analytics/summary${qs ? `?${qs}` : ""}`
    );
  }

  // ── Lineage ────────────────────────────────────────────────────────

  async getLineageByResponse(responseId: string): Promise<LineageRecord[]> {
    return this.request<LineageRecord[]>(
      "GET",
      `/api/km/lineage/response/${responseId}`
    );
  }

  async getLineageByDocument(documentId: string): Promise<LineageRecord[]> {
    return this.request<LineageRecord[]>(
      "GET",
      `/api/km/lineage/document/${documentId}`
    );
  }

  // ── Audit Log ──────────────────────────────────────────────────────

  async exportAuditLog(opts: {
    format?: string;
    fromDate?: string;
    toDate?: string;
    action?: string;
  } = {}): Promise<AuditLogEntry[]> {
    const params = new URLSearchParams();
    params.set("format", opts.format ?? "json");
    if (opts.fromDate) params.set("from", opts.fromDate);
    if (opts.toDate) params.set("to", opts.toDate);
    if (opts.action) params.set("action", opts.action);
    return this.request<AuditLogEntry[]>(
      "GET",
      `/api/km/settings/audit-log/export?${params}`
    );
  }

  async getAuditAnalytics(opts: {
    fromDate?: string;
    toDate?: string;
  } = {}): Promise<AuditAnalytics> {
    const params = new URLSearchParams();
    if (opts.fromDate) params.set("from", opts.fromDate);
    if (opts.toDate) params.set("to", opts.toDate);
    const qs = params.toString();
    return this.request<AuditAnalytics>(
      "GET",
      `/api/km/settings/audit-log/analytics${qs ? `?${qs}` : ""}`
    );
  }

  // ── Tenants ────────────────────────────────────────────────────────

  async listTenants(): Promise<Tenant[]> {
    return this.request<Tenant[]>("GET", "/api/km/tenants");
  }

  async createTenant(name: string, plan: string = "free"): Promise<Tenant> {
    return this.request<Tenant>("POST", "/api/km/tenants", { name, plan });
  }

  async deleteTenant(tenantId: string): Promise<void> {
    await this.request("DELETE", `/api/km/tenants/${tenantId}`);
  }

  // ── Roles ──────────────────────────────────────────────────────────

  async listRoles(): Promise<CustomRole[]> {
    return this.request<CustomRole[]>("GET", "/api/km/roles");
  }

  async createRole(
    name: string,
    description: string = "",
    permissions: Permission[] = []
  ): Promise<CustomRole> {
    return this.request<CustomRole>("POST", "/api/km/roles", {
      name,
      description,
      permissions,
    });
  }

  async deleteRole(roleId: string): Promise<void> {
    await this.request("DELETE", `/api/km/roles/${roleId}`);
  }

  // ── Prompt Marketplace ─────────────────────────────────────────────

  async listPrompts(opts: {
    category?: string;
    search?: string;
  } = {}): Promise<PromptTemplate[]> {
    const params = new URLSearchParams();
    if (opts.category) params.set("category", opts.category);
    if (opts.search) params.set("search", opts.search);
    const qs = params.toString();
    return this.request<PromptTemplate[]>(
      "GET",
      `/api/km/prompts/marketplace${qs ? `?${qs}` : ""}`
    );
  }

  async createPrompt(
    name: string,
    content: string,
    category: string = "general",
    variables: string[] = []
  ): Promise<PromptTemplate> {
    return this.request<PromptTemplate>(
      "POST",
      "/api/km/prompts/marketplace",
      { name, content, category, variables }
    );
  }

  async deletePrompt(promptId: string): Promise<void> {
    await this.request("DELETE", `/api/km/prompts/marketplace/${promptId}`);
  }

  // ── Fine-tuning ────────────────────────────────────────────────────

  async listFinetuneDatasets(): Promise<FinetuneDataset[]> {
    return this.request<FinetuneDataset[]>("GET", "/api/km/finetune/datasets");
  }

  async createFinetuneDataset(
    name: string,
    description: string = ""
  ): Promise<FinetuneDataset> {
    return this.request<FinetuneDataset>(
      "POST",
      "/api/km/finetune/datasets",
      { name, description }
    );
  }

  async listFinetuneJobs(): Promise<FinetuneJob[]> {
    return this.request<FinetuneJob[]>("GET", "/api/km/finetune/jobs");
  }

  // ── Personal Memory ────────────────────────────────────────────────

  async listMemories(userId: string): Promise<Record<string, unknown>[]> {
    return this.request<Record<string, unknown>[]>(
      "GET",
      `/api/km/users/${userId}/memories`
    );
  }

  async deleteMemory(userId: string, memoryId: string): Promise<void> {
    await this.request(
      "DELETE",
      `/api/km/users/${userId}/memories/${memoryId}`
    );
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
