import client from './client';
import type { GuardrailsConfig } from './types';

export interface CodeCount {
  code: string;
  count: number;
}

export interface TimeBucket {
  bucket: string;
  violations: number;
  blocks: number;
}

export interface GuardrailsStats {
  input_checks_total: number;
  output_checks_total: number;
  violations_total: number;
  input_blocks_total: number;
  output_blocks_total: number;
  by_code: CodeCount[];
  buckets: TimeBucket[];
}

export interface ViolationRow {
  timestamp: string;
  response_id: string;
  user_id: string | null;
  workspace_id: string | null;
  query_preview: string;
  codes: string[];
  input_pass: boolean | null;
  output_pass: boolean | null;
}

export interface ViolationsResponse {
  entries: ViolationRow[];
  total: number;
}

export interface GuardrailsFilter {
  workspace_id?: string;
  user_id?: string;
  from?: string;
  to?: string;
  limit?: number;
  offset?: number;
}

export interface PreviewVerdict {
  /** "pass" | "sanitize" | "block" | "regenerate" */
  action: string;
  codes: string[];
  output: string | null;
}

export interface PreviewResponse {
  input: PreviewVerdict | null;
  output: PreviewVerdict | null;
}

export interface PreviewRequest {
  query?: string;
  response?: string;
  policy?: GuardrailsConfig;
}

function buildQuery(f: GuardrailsFilter): string {
  const params = new URLSearchParams();
  if (f.workspace_id) params.set('workspace_id', f.workspace_id);
  if (f.user_id) params.set('user_id', f.user_id);
  if (f.from) params.set('from', f.from);
  if (f.to) params.set('to', f.to);
  if (f.limit !== undefined) params.set('limit', String(f.limit));
  if (f.offset !== undefined) params.set('offset', String(f.offset));
  const s = params.toString();
  return s ? `?${s}` : '';
}

export async function getGuardrailsStats(filter: GuardrailsFilter = {}) {
  const res = await client.get<GuardrailsStats>(
    `/api/km/guardrails/stats${buildQuery(filter)}`,
  );
  return res.data;
}

export async function listGuardrailViolations(filter: GuardrailsFilter = {}) {
  const res = await client.get<ViolationsResponse>(
    `/api/km/guardrails/violations${buildQuery(filter)}`,
  );
  return res.data;
}

export async function previewGuardrails(req: PreviewRequest) {
  const res = await client.post<PreviewResponse>('/api/km/guardrails/preview', req);
  return res.data;
}
