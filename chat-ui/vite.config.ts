import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// The chat UI is a separate first-party app from admin-ui (different audience:
// end users vs admins). Dev server on 8082; all backend calls proxy to :8080.
export default defineConfig({
  plugins: [react()],
  server: {
    port: 8082,
    proxy: {
      '/api': 'http://localhost:8080',
      '/v1': 'http://localhost:8080',
      '/health': 'http://localhost:8080',
    },
  },
});
