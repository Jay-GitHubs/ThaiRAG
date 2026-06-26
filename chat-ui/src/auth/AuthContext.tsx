import React, { createContext, useCallback, useContext, useState } from 'react';
import { login as apiLogin } from '../api/auth';
import { setToken, getToken } from '../api/client';
import type { User } from '../api/types';

const USER_KEY = 'thairag-chat-user';

interface AuthState {
  user: User | null;
  isAuthenticated: boolean;
  login: (email: string, password: string) => Promise<void>;
  loginWithToken: (token: string, user: User) => void;
  logout: () => void;
}

const AuthContext = createContext<AuthState>({
  user: null,
  isAuthenticated: false,
  login: async () => {},
  loginWithToken: () => {},
  logout: () => {},
});

function loadUser(): User | null {
  if (!getToken()) return null;
  try {
    const raw = sessionStorage.getItem(USER_KEY);
    return raw ? (JSON.parse(raw) as User) : null;
  } catch {
    return null;
  }
}

export function AuthProvider({ children }: { children: React.ReactNode }) {
  const [user, setUser] = useState<User | null>(loadUser);

  const login = useCallback(async (email: string, password: string) => {
    const res = await apiLogin({ email, password });
    setToken(res.token);
    sessionStorage.setItem(USER_KEY, JSON.stringify(res.user));
    setUser(res.user);
  }, []);

  // Used by the OIDC/SSO callback, which delivers a ready JWT + user via the URL.
  const loginWithToken = useCallback((token: string, u: User) => {
    setToken(token);
    sessionStorage.setItem(USER_KEY, JSON.stringify(u));
    setUser(u);
  }, []);

  const logout = useCallback(() => {
    setToken(null);
    sessionStorage.removeItem(USER_KEY);
    setUser(null);
    window.location.href = '/login';
  }, []);

  return (
    <AuthContext.Provider value={{ user, isAuthenticated: !!user, login, loginWithToken, logout }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth(): AuthState {
  return useContext(AuthContext);
}
