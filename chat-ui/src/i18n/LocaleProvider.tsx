/**
 * Minimal locale system for the chat UI chrome (English / Thai), mirroring the
 * homegrown ThemeProvider pattern rather than pulling in an i18n framework:
 * two typed catalogs, a context, and a `t()` with `{var}` interpolation.
 *
 * Scope note: this localizes the UI *chrome* only. Answer-derived content
 * (confidence summary/factors, citations) already arrives localized from the
 * backend in the ANSWER's language, and the confidence chrome deliberately
 * follows the answer, not the UI locale — see MessageBubble.
 */
import { createContext, useCallback, useContext, useMemo, useState } from 'react';
import type { ReactNode } from 'react';
import { en } from './en';
import { th } from './th';

export type Locale = 'en' | 'th';
export type MessageKey = keyof typeof en;

const CATALOGS: Record<Locale, Record<MessageKey, string>> = { en, th };
const STORAGE_KEY = 'thairag-chat-locale';

function detectLocale(): Locale {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved === 'en' || saved === 'th') return saved;
  } catch {
    /* storage unavailable — fall through to browser language */
  }
  return navigator.language?.toLowerCase().startsWith('th') ? 'th' : 'en';
}

interface LocaleContextValue {
  locale: Locale;
  setLocale: (l: Locale) => void;
  t: (key: MessageKey, vars?: Record<string, string | number>) => string;
}

const LocaleContext = createContext<LocaleContextValue>({
  locale: 'en',
  setLocale: () => {},
  t: (key) => en[key],
});

export function LocaleProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(detectLocale);

  const setLocale = useCallback((l: Locale) => {
    try {
      localStorage.setItem(STORAGE_KEY, l);
    } catch {
      /* non-fatal: locale just won't persist */
    }
    setLocaleState(l);
  }, []);

  const t = useCallback(
    (key: MessageKey, vars?: Record<string, string | number>) => {
      let msg = CATALOGS[locale][key] ?? en[key];
      if (vars) {
        for (const [k, v] of Object.entries(vars)) {
          msg = msg.replace(`{${k}}`, String(v));
        }
      }
      return msg;
    },
    [locale],
  );

  const value = useMemo(() => ({ locale, setLocale, t }), [locale, setLocale, t]);
  return <LocaleContext.Provider value={value}>{children}</LocaleContext.Provider>;
}

/** Locale + translate function. `t('key')` or `t('key', { var: value })`. */
export function useI18n() {
  return useContext(LocaleContext);
}
