import client from './client';

export interface PluginInfo {
  name: string;
  description: string;
  plugin_type: string;
  enabled: boolean;
}

export interface PluginListResponse {
  plugins: PluginInfo[];
}

export interface PluginActionResponse {
  name: string;
  enabled: boolean;
  message: string;
}

export async function listPlugins(): Promise<PluginListResponse> {
  const res = await client.get('/api/km/plugins');
  return res.data;
}

export async function enablePlugin(name: string): Promise<PluginActionResponse> {
  const res = await client.post(`/api/km/plugins/${name}/enable`);
  return res.data;
}

export async function disablePlugin(name: string): Promise<PluginActionResponse> {
  const res = await client.post(`/api/km/plugins/${name}/disable`);
  return res.data;
}
