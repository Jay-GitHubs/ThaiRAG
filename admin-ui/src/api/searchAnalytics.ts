import client from './client';

export interface SearchAnalyticsSummary {
  total_searches: number;
  zero_result_rate: number;
  avg_latency_ms: number;
  avg_results: number;
}

export interface PopularQuery {
  query: string;
  count: number;
  avg_results: number;
  avg_latency_ms: number;
}

export interface SearchAnalyticsEvent {
  id: string;
  query: string;
  result_count: number;
  latency_ms: number;
  timestamp: string;
  workspace_id?: string;
  user_id?: string;
}

export interface SearchAnalyticsFilter {
  from?: string;
  to?: string;
  limit?: number;
  zero_results_only?: boolean;
}

export async function getSearchAnalyticsSummary(filter?: Pick<SearchAnalyticsFilter, 'from' | 'to'>): Promise<SearchAnalyticsSummary> {
  const res = await client.get<SearchAnalyticsSummary>('/api/km/search-analytics/summary', { params: filter });
  return res.data;
}

export async function getPopularQueries(limit = 20, filter?: Pick<SearchAnalyticsFilter, 'from' | 'to'>): Promise<PopularQuery[]> {
  const res = await client.get<PopularQuery[]>('/api/km/search-analytics/popular', { params: { limit, ...filter } });
  return res.data;
}

export async function getSearchAnalyticsEvents(filter?: SearchAnalyticsFilter): Promise<SearchAnalyticsEvent[]> {
  const res = await client.get<SearchAnalyticsEvent[]>('/api/km/search-analytics/events', { params: filter });
  return res.data;
}
