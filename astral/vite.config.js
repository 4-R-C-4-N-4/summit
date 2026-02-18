import react from '@vitejs/plugin-react'
import {defineConfig} from 'vite'

export default defineConfig({
  plugins : [ react() ],
  base : '/',
  build : {
    outDir : 'dist',
    assetsDir : 'assets',
    emptyOutDir : true,
  },
  server : {
    proxy : {
      '/api' : {
        target : 'http://127.0.0.1:9001',
        changeOrigin : true,
        rewrite : (path) => path.replace(/^\/api/, '')
      }
    }
  }
})
