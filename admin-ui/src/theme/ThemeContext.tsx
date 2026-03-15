import React, { createContext, useCallback, useContext, useEffect, useState } from 'react';

type ThemeMode = 'light' | 'dark';

interface ThemeCtx {
  mode: ThemeMode;
  toggle: () => void;
}

const STORAGE_KEY = 'thairag-theme';

const ThemeContext = createContext<ThemeCtx>({ mode: 'light', toggle: () => {} });

function getInitialMode(): ThemeMode {
  const stored = localStorage.getItem(STORAGE_KEY);
  if (stored === 'dark' || stored === 'light') return stored;
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const [mode, setMode] = useState<ThemeMode>(getInitialMode);

  const toggle = useCallback(() => {
    setMode((prev) => (prev === 'light' ? 'dark' : 'light'));
  }, []);

  useEffect(() => {
    localStorage.setItem(STORAGE_KEY, mode);
    document.body.style.background = mode === 'dark' ? '#141414' : '#ffffff';
  }, [mode]);

  return <ThemeContext.Provider value={{ mode, toggle }}>{children}</ThemeContext.Provider>;
}

export function useThemeMode() {
  return useContext(ThemeContext);
}
