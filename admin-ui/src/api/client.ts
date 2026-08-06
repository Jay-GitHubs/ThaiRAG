import axios from 'axios';

const TOKEN_KEY = 'thairag-token';

function loadToken(): string | null {
  return sessionStorage.getItem(TOKEN_KEY);
}

export function setToken(t: string | null) {
  if (t) {
    sessionStorage.setItem(TOKEN_KEY, t);
  } else {
    sessionStorage.removeItem(TOKEN_KEY);
  }
}

export function getToken(): string | null {
  return loadToken();
}

const client = axios.create({
  baseURL: '',
  headers: { 'Content-Type': 'application/json' },
});

client.interceptors.request.use((config) => {
  const token = loadToken();
  if (token) {
    config.headers.Authorization = `Bearer ${token}`;
  }
  return config;
});

client.interceptors.response.use(
  (res) => res,
  (error) => {
    if (error.response?.status === 401) {
      setToken(null);
      // Don't redirect if already on login page — let the login form show the error
      if (!window.location.pathname.startsWith('/login')) {
        window.location.href = '/login';
      }
    }
    return Promise.reject(error);
  },
);


/**
 * Extract a human-readable message from an API error. The backend returns
 * `{ error: { message } }`; fall back to the axios/network message, then a
 * caller-supplied default. Use this instead of swallowing errors into a
 * generic string so operators can see what actually failed.
 */
export function apiErrorMessage(err: unknown, fallback = 'Request failed'): string {
  if (axios.isAxiosError(err)) {
    const serverMsg = (err.response?.data as { error?: { message?: string } } | undefined)?.error
      ?.message;
    if (serverMsg) return serverMsg;
    if (err.code === 'ECONNABORTED') return 'The request timed out.';
    if (!err.response) return 'Could not reach the server.';
    if (err.message) return err.message;
  }
  if (err instanceof Error && err.message) return err.message;
  return fallback;
}

export default client;
