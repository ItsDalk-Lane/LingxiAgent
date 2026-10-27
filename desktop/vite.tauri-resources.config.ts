import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import path from 'node:path';

export default defineConfig({
  root: path.resolve(import.meta.dirname, 'src/tauri-resources'),
  base: './',
  plugins: [react()],
  build: { outDir: path.resolve(import.meta.dirname, 'dist-tauri-resources'), emptyOutDir: true },
  test: { environment: 'jsdom', include: ['__tests__/**/*.test.tsx'] },
});
