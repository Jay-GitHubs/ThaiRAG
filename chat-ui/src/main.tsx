import React from 'react';
import ReactDOM from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { BrowserRouter } from 'react-router-dom';
import './index.css';
import { AuthProvider } from './auth/AuthContext';
import { ThemeProvider } from './theme/ThemeProvider';
import { LocaleProvider } from './i18n/LocaleProvider';
import { App } from './App';

const queryClient = new QueryClient({
  defaultOptions: {
    queries: { retry: 1, refetchOnWindowFocus: false },
  },
});

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <LocaleProvider>
        <ThemeProvider>
          <BrowserRouter>
            <AuthProvider>
              <App />
            </AuthProvider>
          </BrowserRouter>
        </ThemeProvider>
      </LocaleProvider>
    </QueryClientProvider>
  </React.StrictMode>,
);
