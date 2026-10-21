import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { readFile, lstat, realpath } from 'node:fs/promises';
import { createHash } from 'node:crypto';

const root = path.dirname(fileURLToPath(import.meta.url));
const policy = [
  "default-src 'none'", "script-src 'self' 'wasm-unsafe-eval'",
  "style-src 'self' 'unsafe-inline'", "img-src 'self' blob: data:",
  "font-src 'self' blob: data:", "worker-src 'self' blob:",
  "object-src 'none'", "base-uri 'none'", "form-action 'none'",
];
const headers = (development: boolean) => ({
  'Content-Security-Policy': [...policy,
    development ? "connect-src 'self' ipc: http://ipc.localhost https://ipc.localhost ws://127.0.0.1:43182" : "connect-src 'self'",
    "frame-ancestors 'none'"].join('; '),
  'X-Content-Type-Options': 'nosniff',
  'Referrer-Policy': 'no-referrer',
});

export default defineConfig({
  plugins: [svelte(), {
    name: 'development-reading-sample', apply: 'serve',
    configureServer(server) {
      server.middlewares.use('/__samples', async (request, response) => {
        // Exactly one approved synthetic fixture, development only; no path API.
        if (request.url !== '/geometry.pdf' || !['GET', 'HEAD'].includes(request.method || '')) {
          response.statusCode = 404; response.end(); return;
        }
        try {
          const filename = path.join(root, 'tests/fixtures/g1/geometry.pdf');
          if (!(await lstat(filename)).isFile() ||
              path.normalize(await realpath(filename)).toLowerCase() !== filename.toLowerCase()) throw new Error('Linked sample');
          const bytes = await readFile(filename);
          if (createHash('sha256').update(bytes).digest('hex') !==
              '1917dff0058a73a311bc820623a68b1c36edbaf1a394b5782e7df4092df1484f') throw new Error('Unexpected sample');
          response.setHeader('Content-Type', 'application/pdf');
          response.setHeader('Cache-Control', 'no-store');
          response.end(request.method === 'HEAD' ? undefined : bytes);
        } catch { response.statusCode = 404; response.end(); }
      });
    },
  }],
  cacheDir: '.cache/vite',
  server: {
    host: '127.0.0.1', port: 43182, strictPort: true, cors: false,
    allowedHosts: ['127.0.0.1'], headers: headers(true),
    watch: { ignored: ['**/.cache/**', '**/src-tauri/**', '**/tests/work/**', '**/spikes/**'] },
    fs: { strict: true,
      allow: ['src', 'node_modules', 'public', '.cache/vite'].map(p => path.join(root, p)),
      deny: ['.env', '.env.*', '**/.git/**', '**/tests/**', '**/spikes/**', '**/*.md'],
    },
  },
  preview: { host: '127.0.0.1', port: 43182, strictPort: true,
    allowedHosts: ['127.0.0.1'], headers: headers(false) },
  build: { target: 'es2022', sourcemap: false },
});
