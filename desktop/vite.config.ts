import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { target: 'chrome110', sourcemap: true },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test/setup.ts'],
    passWithNoTests: true,
    include: ['src/**/*.test.{ts,tsx}'],
  },
});
