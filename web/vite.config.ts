import { resolve } from 'node:path'
import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// 开发时把 /api 代理到 `tmuxide serve --port 7878`
const target = process.env.TMUXIDE_DEV_SERVER ?? 'http://127.0.0.1:7878'

export default defineConfig({
  plugins: [vue()],
  base: './',
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    chunkSizeWarningLimit: 1200,
    assetsInlineLimit: 0,
    // index.html：电脑界面；m.html：远程端口上的手机界面
    rollupOptions: { input: { main: resolve(__dirname, 'index.html'), m: resolve(__dirname, 'm.html') } },
  },
  server: {
    proxy: { '/api': { target, ws: true, changeOrigin: true } },
  },
})
