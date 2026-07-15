import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

  // 'mpa' disables the dev/preview SPA fallback so a missing
  // /inventory.snapshot.json returns a real 404 (the SPA fallback would serve
  // index.html with 200, making the loader misreport 'invalid' instead of 'none').
  // FORWARD RISK: if a later phase adds client-side routes (e.g. /parts/123),
  // direct navigation will 404 under 'mpa'. Fix then with preview/server
  // middleware that 404s only asset-like paths instead of disabling the
  // fallback wholesale.
export default defineConfig({ plugins: [react()], appType: 'mpa' });
