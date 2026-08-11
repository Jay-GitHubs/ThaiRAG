import client from './client';

export interface Branding {
  app_name: string;
  logo_data_url: string | null;
}

/** Public — no auth needed (login screens read it). */
export async function getBranding(): Promise<Branding> {
  const res = await client.get<Branding>('/api/branding');
  return res.data;
}

export async function updateBranding(b: Branding): Promise<Branding> {
  const res = await client.put<Branding>('/api/km/settings/branding', b);
  return res.data;
}
