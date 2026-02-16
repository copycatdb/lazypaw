import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'path'

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      'lazypaw-js': path.resolve(__dirname, '../sdk/src/index.ts'),
    },
  },
  server: {
    host: '0.0.0.0',
    port: 5173,
  },
})
