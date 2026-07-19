import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

// jsdom environment (mirroring @ei/desktop's vitest config) because Task 7
// adds component tests (Inventory table, App states) alongside the original
// pure-function tests -- which run unmodified under jsdom.
export default defineConfig({
  plugins: [react()],
  test: { environment: 'jsdom', include: ['src/**/*.test.{ts,tsx}'] },
});
