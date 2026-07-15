import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// appType: 'mpa' disables Vite's SPA history-fallback middleware (both dev and
// `vite preview`), so a request for a missing static file like
// /inventory.snapshot.json returns a real 404 instead of index.html. Without
// this, loadSnapshot() sees `res.ok === true` with an HTML body and reports
// 'invalid' instead of 'none' for the "no snapshot published" case.
export default defineConfig({ plugins: [react()], appType: 'mpa' });
