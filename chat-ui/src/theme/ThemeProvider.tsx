import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { ConfigProvider, theme as antdTheme } from 'antd';

type Mode = 'light' | 'dark';
const STORAGE_KEY = 'thairag-theme';

const FONT_STACK =
  "'IBM Plex Sans Thai', 'IBM Plex Sans', -apple-system, BlinkMacSystemFont, sans-serif";

// Drive antd from the same Celadon & Ink tokens as index.css so the whole app
// reads as one identity in either theme — never default antd blue.
function tokens(mode: Mode) {
  const base = { fontFamily: FONT_STACK, borderRadius: 10 };
  return mode === 'dark'
    ? {
        ...base,
        colorPrimary: '#46b3a0',
        colorInfo: '#46b3a0',
        colorLink: '#7fd3c1',
        colorText: '#e7edf6',
        colorTextSecondary: '#97a4b8',
        colorBgLayout: '#111d31',
        colorBgContainer: '#18263f',
        colorBgElevated: '#1b2c47',
        colorBorder: '#283750',
      }
    : {
        ...base,
        colorPrimary: '#2f8e7e',
        colorInfo: '#2f8e7e',
        colorLink: '#246b5f',
        colorText: '#1a2330',
        colorTextSecondary: '#5b6675',
        colorBgLayout: '#faf8f3',
      };
}

function initialMode(): Mode {
  const saved = typeof localStorage !== 'undefined' ? localStorage.getItem(STORAGE_KEY) : null;
  if (saved === 'light' || saved === 'dark') return saved;
  return window.matchMedia?.('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

const ThemeContext = createContext<{ mode: Mode; toggle: () => void }>({
  mode: 'light',
  toggle: () => {},
});

// eslint-disable-next-line react-refresh/only-export-components
export const useTheme = () => useContext(ThemeContext);

/** App-wide light/dark theming: persists an explicit choice, follows the OS
 *  until the user makes one, and reflects the active mode onto the documentElement
 *  (`data-theme`) so the CSS variables in index.css switch with it. */
export function ThemeProvider({ children }: { children: ReactNode }) {
  const [mode, setMode] = useState<Mode>(initialMode);

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', mode);
  }, [mode]);

  // Track the OS preference only while the user hasn't pinned a choice.
  useEffect(() => {
    if (localStorage.getItem(STORAGE_KEY)) return;
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    const onChange = (e: MediaQueryListEvent) => setMode(e.matches ? 'dark' : 'light');
    mq.addEventListener('change', onChange);
    return () => mq.removeEventListener('change', onChange);
  }, []);

  const toggle = () =>
    setMode((m) => {
      const next: Mode = m === 'dark' ? 'light' : 'dark';
      localStorage.setItem(STORAGE_KEY, next);
      return next;
    });

  const value = useMemo(() => ({ mode, toggle }), [mode]);
  const algorithm = mode === 'dark' ? antdTheme.darkAlgorithm : antdTheme.defaultAlgorithm;

  return (
    <ThemeContext.Provider value={value}>
      <ConfigProvider theme={{ algorithm, token: tokens(mode) }}>{children}</ConfigProvider>
    </ThemeContext.Provider>
  );
}
