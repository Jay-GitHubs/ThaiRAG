import client from './client';

export interface PromptTemplate {
  id: string;
  name: string;
  description: string;
  category: string;
  content: string;
  variables: string[];
  rating_avg: number;
  rating_count: number;
}

/** Public prompt templates from the marketplace, for the composer picker.
 *  Errors (e.g. a chat-only account without /api/km access) resolve to an
 *  empty list so the picker simply stays hidden. */
export async function listPromptTemplates(): Promise<PromptTemplate[]> {
  try {
    const res = await client.get<PromptTemplate[]>('/api/km/prompts/marketplace', {
      params: { is_public: true, limit: 50 },
    });
    return res.data;
  } catch {
    return [];
  }
}
