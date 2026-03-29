import client from './client';

export interface AuditLogEntry {
  id: string;
  timestamp: string;
  user_id?: string;
  user_email?: string;
  action: string;
  detail: string;
  success: boolean;
  ip_address?: string;
}

export interface AuditAnalytics {
  actions_by_type: [string, number][];
  actions_by_user: [string, number][];
  events_per_day: [string, number][];
  total_events: number;
  success_rate?: number;
}

export interface AuditLogFilter {
  from?: string;
  to?: string;
  action?: string;
  user_id?: string;
  format?: 'json' | 'csv';
}

export async function exportAuditLog(filter?: AuditLogFilter): Promise<AuditLogEntry[] | string> {
  const res = await client.get('/api/km/settings/audit-log/export', { params: filter });
  return res.data;
}

export async function getAuditAnalytics(filter?: Pick<AuditLogFilter, 'from' | 'to'>): Promise<AuditAnalytics> {
  const res = await client.get<AuditAnalytics>('/api/km/settings/audit-log/analytics', { params: filter });
  return res.data;
}
