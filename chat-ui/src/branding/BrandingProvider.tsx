import { createContext, useContext, useEffect, useState, type ReactNode } from 'react';
import { getBranding, type Branding } from '../api/branding';

const DEFAULT: Branding = { app_name: 'ThaiRAG', logo_data_url: null };
const Ctx = createContext<Branding>(DEFAULT);

/** White-label branding for the chat app, read from the same public
 *  /api/branding the admin console writes. Applies the browser title + favicon. */
export function BrandingProvider({ children }: { children: ReactNode }) {
  const [branding, setBranding] = useState<Branding>(DEFAULT);

  useEffect(() => {
    getBranding()
      .then(setBranding)
      .catch(() => setBranding(DEFAULT));
  }, []);

  useEffect(() => {
    document.title = `${branding.app_name} Chat`;
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

  return <Ctx.Provider value={branding}>{children}</Ctx.Provider>;
}

export function useBranding() {
  return useContext(Ctx);
}
