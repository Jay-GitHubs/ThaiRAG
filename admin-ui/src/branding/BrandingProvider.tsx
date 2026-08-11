import { createContext, useContext, useEffect, useState, type ReactNode } from 'react';
import { getBranding, type Branding } from '../api/branding';

const DEFAULT: Branding = { app_name: 'ThaiRAG', logo_data_url: null };

interface BrandingCtx extends Branding {
  refresh: () => Promise<void>;
}

const Ctx = createContext<BrandingCtx>({ ...DEFAULT, refresh: async () => {} });

/** App-wide white-label branding. Fetched once (public endpoint), applied to
 *  the document title + favicon, and exposed to headers/login. */
export function BrandingProvider({ children }: { children: ReactNode }) {
  const [branding, setBranding] = useState<Branding>(DEFAULT);

  const load = async () => {
    try {
      setBranding(await getBranding());
    } catch {
      setBranding(DEFAULT); // network error → safe default, never blocks the app
    }
  };

  useEffect(() => {
    void load();
  }, []);

  useEffect(() => {
    document.title = `${branding.app_name} Admin`;
    if (branding.logo_data_url) {
      let link = document.querySelector<HTMLLinkElement>('link[rel="icon"]');
      if (!link) {
        link = document.createElement('link');
        link.rel = 'icon';
        document.head.appendChild(link);
      }
      link.href = branding.logo_data_url;
    }
  }, [branding]);

  return <Ctx.Provider value={{ ...branding, refresh: load }}>{children}</Ctx.Provider>;
}

export function useBranding() {
  return useContext(Ctx);
}
