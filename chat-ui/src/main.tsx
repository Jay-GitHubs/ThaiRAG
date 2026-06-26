import React from 'react';
import ReactDOM from 'react-dom/client';
import { ConfigProvider, theme as antdTheme } from 'antd';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { BrowserRouter } from 'react-router-dom';
import './index.css';
import { AuthProvider } from './auth/AuthContext';
import { App } from './App';

const queryClient = new QueryClient({
  defaultOptions: {
    queries: { retry: 1, refetchOnWindowFocus: false },
  },
});

const FONT_STACK =
  "'IBM Plex Sans Thai', 'IBM Plex Sans', -apple-system, BlinkMacSystemFont, sans-serif";

// Celadon & Ink — drive antd from the same tokens as index.css so the whole
// app reads as one deliberate identity rather than default antd blue.
const thairagTheme = {
  algorithm: antdTheme.defaultAlgorithm,
  token: {
    colorPrimary: '#2f8e7e',
    colorInfo: '#2f8e7e',
    colorLink: '#246b5f',
    colorText: '#1a2330',
    colorTextSecondary: '#5b6675',
    fontFamily: FONT_STACK,
    borderRadius: 10,
    colorBgLayout: '#faf8f3',
  },
};

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <ConfigProvider theme={thairagTheme}>
        <BrowserRouter>
          <AuthProvider>
            <App />
          </AuthProvider>
        </BrowserRouter>
      </ConfigProvider>
    </QueryClientProvider>
  </React.StrictMode>,
);
