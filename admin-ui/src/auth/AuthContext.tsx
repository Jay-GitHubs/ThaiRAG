import React, { createContext, useCallback, useState } from 'react';
import { login as apiLogin } from '../api/auth';
import { setToken, getToken } from '../api/client';
import type { User } from '../api/types';

const USER_KEY = 'thairag-user';

interface AuthState {
  user: User | null;
  isAuthenticated: boolean;
  login: (email: string, password: string) => Promise<void>;
  loginWithToken: (token: string, user: User) => void;
  logout: () => void;
}

export const AuthContext = createContext<AuthState>({
  user: null,
  isAuthenticated: false,
  login: async () => {},
  loginWithToken: () => {},
  logout: () => {},
});

function loadUser(): User | null {
  const token = getToken();
  if (!token) return null;
  try {
    const raw = sessionStorage.getItem(USER_KEY);
    return raw ? JSON.parse(raw) : null;
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

  const loginWithToken = useCallback((token: string, user: User) => {
    setToken(token);
    sessionStorage.setItem(USER_KEY, JSON.stringify(user));
    setUser(user);
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
