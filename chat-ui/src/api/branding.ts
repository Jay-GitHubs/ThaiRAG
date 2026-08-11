import client from './client';

export interface Branding {
  app_name: string;
  logo_data_url: string | null;
}

/** Public — no auth (login screen reads it too). */
export async function getBranding(): Promise<Branding> {
  const res = await client.get<Branding>('/api/branding');
  return res.data;
}
