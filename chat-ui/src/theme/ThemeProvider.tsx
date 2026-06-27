import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { ConfigProvider, theme as antdTheme } from 'antd';

export type ThemeId =
  | 'celadon'
  | 'ink'
  | 'lanna'
  | 'cinnabar'
  | 'sakura'
  | 'daylight'
  | 'nebula'
  | 'synthwave'
  | 'amber'
  | 'aurora';

const STORAGE_KEY = 'thairag-theme';

const FONT_STACK =
  "'IBM Plex Sans Thai', 'IBM Plex Sans', -apple-system, BlinkMacSystemFont, sans-serif";
const MONO_STACK = "'IBM Plex Mono', ui-monospace, SFMono-Regular, Menlo, monospace";

export interface ThemeDef {
  id: ThemeId;
  label: string;
  group: 'Light' | 'Dark';
  dark: boolean;
  /** Two-color chip for the picker: the reading canvas + the brand accent. */
  swatch: { canvas: string; accent: string };
  /** antd tokens — kept in lockstep with the CSS variables in index.css so
   *  antd's own components (buttons, inputs, modals, tags…) match the theme. */
  antd: {
    colorPrimary: string;
    colorLink: string;
    colorBgLayout: string;
    colorBgContainer: string;
    colorBgElevated: string;
    colorText: string;
    colorTextSecondary: string;
    colorBorder: string;
    fontFamily?: string;
  };
}

// Every theme defined once. The CSS-variable side lives in index.css under the
// matching :root[data-theme='<id>'] block; these are the antd-side mirrors.
export const THEMES: ThemeDef[] = [
  {
    id: 'celadon',
    label: 'Celadon',
    group: 'Light',
    dark: false,
    swatch: { canvas: '#faf8f3', accent: '#2f8e7e' },
    antd: {
      colorPrimary: '#2f8e7e',
      colorLink: '#246b5f',
      colorBgLayout: '#faf8f3',
      colorBgContainer: '#ffffff',
      colorBgElevated: '#ffffff',
      colorText: '#1a2330',
      colorTextSecondary: '#5b6675',
      colorBorder: '#e9e3d7',
    },
  },
  {
    id: 'lanna',
    label: 'Lanna Indigo',
    group: 'Light',
    dark: false,
    swatch: { canvas: '#f6f3ec', accent: '#3a4f9c' },
    antd: {
      colorPrimary: '#3a4f9c',
      colorLink: '#2c3d7d',
      colorBgLayout: '#f6f3ec',
      colorBgContainer: '#fffdf8',
      colorBgElevated: '#fffdf8',
      colorText: '#20242e',
      colorTextSecondary: '#5d6373',
      colorBorder: '#e4ddcd',
    },
  },
  {
    id: 'cinnabar',
    label: 'Cinnabar',
    group: 'Light',
    dark: false,
    swatch: { canvas: '#f7f2ef', accent: '#c0392b' },
    antd: {
      colorPrimary: '#c0392b',
      colorLink: '#9e2b20',
      colorBgLayout: '#f7f2ef',
      colorBgContainer: '#fffdfb',
      colorBgElevated: '#fffdfb',
      colorText: '#2a2020',
      colorTextSecondary: '#6b5b56',
      colorBorder: '#ecdfd8',
    },
  },
  {
    id: 'sakura',
    label: 'Sakura',
    group: 'Light',
    dark: false,
    swatch: { canvas: '#fbf3f5', accent: '#d56a93' },
    antd: {
      colorPrimary: '#d56a93',
      colorLink: '#b24e78',
      colorBgLayout: '#fbf3f5',
      colorBgContainer: '#fffafc',
      colorBgElevated: '#fffafc',
      colorText: '#3a2b33',
      colorTextSecondary: '#7a6470',
      colorBorder: '#f0dde4',
    },
  },
  {
    id: 'daylight',
    label: 'Daylight',
    group: 'Light',
    dark: false,
    swatch: { canvas: '#f8fafc', accent: '#2563eb' },
    antd: {
      colorPrimary: '#2563eb',
      colorLink: '#1d4ed8',
      colorBgLayout: '#f8fafc',
      colorBgContainer: '#ffffff',
      colorBgElevated: '#ffffff',
      colorText: '#1e293b',
      colorTextSecondary: '#64748b',
      colorBorder: '#e2e8f0',
    },
  },
  {
    id: 'ink',
    label: 'Ink',
    group: 'Dark',
    dark: true,
    swatch: { canvas: '#111d31', accent: '#46b3a0' },
    antd: {
      colorPrimary: '#46b3a0',
      colorLink: '#7fd3c1',
      colorBgLayout: '#111d31',
      colorBgContainer: '#18263f',
      colorBgElevated: '#1b2c47',
      colorText: '#e7edf6',
      colorTextSecondary: '#97a4b8',
      colorBorder: '#283750',
    },
  },
  {
    id: 'nebula',
    label: 'Nebula',
    group: 'Dark',
    dark: true,
    swatch: { canvas: '#110d24', accent: '#22d3ee' },
    antd: {
      colorPrimary: '#22d3ee',
      colorLink: '#67e8f9',
      colorBgLayout: '#110d24',
      colorBgContainer: '#1a1535',
      colorBgElevated: '#1f1940',
      colorText: '#e9e6fb',
      colorTextSecondary: '#9b93c4',
      colorBorder: '#2e2752',
    },
  },
  {
    id: 'synthwave',
    label: 'Synthwave',
    group: 'Dark',
    dark: true,
    swatch: { canvas: '#1a0e2e', accent: '#ff2e97' },
    antd: {
      colorPrimary: '#ff2e97',
      colorLink: '#ff6ec7',
      colorBgLayout: '#1a0e2e',
      colorBgContainer: '#251539',
      colorBgElevated: '#2d1845',
      colorText: '#ffe6fb',
      colorTextSecondary: '#c09cc9',
      colorBorder: '#3d2257',
    },
  },
  {
    id: 'amber',
    label: 'Amber Terminal',
    group: 'Dark',
    dark: true,
    swatch: { canvas: '#0d0c0a', accent: '#ffb000' },
    antd: {
      colorPrimary: '#ffb000',
      colorLink: '#ffc94d',
      colorBgLayout: '#0d0c0a',
      colorBgContainer: '#16140f',
      colorBgElevated: '#1c1913',
      colorText: '#efe4cf',
      colorTextSecondary: '#a8946e',
      colorBorder: '#2c2719',
      fontFamily: MONO_STACK,
    },
  },
  {
    id: 'aurora',
    label: 'Aurora',
    group: 'Dark',
    dark: true,
    swatch: { canvas: '#0a1f22', accent: '#34d399' },
    antd: {
      colorPrimary: '#34d399',
      colorLink: '#6ee7b7',
      colorBgLayout: '#0a1f22',
      colorBgContainer: '#103034',
      colorBgElevated: '#143b3b',
      colorText: '#dff4ee',
      colorTextSecondary: '#84a8a2',
      colorBorder: '#1f4a4a',
    },
  },
];

const BY_ID = new Map(THEMES.map((t) => [t.id, t]));

function resolveStored(raw: string | null): ThemeId | null {
  if (!raw) return null;
  if (raw === 'light') return 'celadon'; // legacy values from the 2-state toggle
  if (raw === 'dark') return 'ink';
  return BY_ID.has(raw as ThemeId) ? (raw as ThemeId) : null;
}

function initialTheme(): ThemeId {
  const stored = resolveStored(
    typeof localStorage !== 'undefined' ? localStorage.getItem(STORAGE_KEY) : null,
  );
  if (stored) return stored;
  return window.matchMedia?.('(prefers-color-scheme: dark)').matches ? 'ink' : 'celadon';
}

const ThemeContext = createContext<{
  themeId: ThemeId;
  theme: ThemeDef;
  themes: ThemeDef[];
  setTheme: (id: ThemeId) => void;
}>({
  themeId: 'celadon',
  theme: THEMES[0],
  themes: THEMES,
  setTheme: () => {},
});

// eslint-disable-next-line react-refresh/only-export-components
export const useTheme = () => useContext(ThemeContext);

/** App-wide theming across 10 themes. Reflects the active id onto the
 *  documentElement (`data-theme`) so the index.css variables switch, and feeds
 *  the matching tokens to antd so its components stay in sync. */
export function ThemeProvider({ children }: { children: ReactNode }) {
  const [themeId, setThemeId] = useState<ThemeId>(initialTheme);
  const theme = BY_ID.get(themeId) ?? THEMES[0];

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', themeId);
  }, [themeId]);

  const setTheme = (id: ThemeId) => {
    localStorage.setItem(STORAGE_KEY, id);
    setThemeId(id);
  };

  const value = useMemo(
    () => ({ themeId, theme, themes: THEMES, setTheme }),
    [themeId, theme],
  );

  return (
    <ThemeContext.Provider value={value}>
      <ConfigProvider
        theme={{
          algorithm: theme.dark ? antdTheme.darkAlgorithm : antdTheme.defaultAlgorithm,
          token: { fontFamily: FONT_STACK, borderRadius: 10, ...theme.antd },
        }}
      >
        {children}
      </ConfigProvider>
    </ThemeContext.Provider>
  );
}
