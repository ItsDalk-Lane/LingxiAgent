// 隔离真实 Electron，加载生产 settings.html 与 SettingsApp；只有宿主连接桥来自合成服务。
import { createServer } from 'vite';
import { spawn } from 'node:child_process';
import { createServer as createSocketServer } from 'node:net';
import path from 'node:path';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';
const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, '../../../../..');
const home = process.env.R06_SETTINGS_HOME;
if (!home || !process.env.R06_SETTINGS_URL || !process.env.R06_SETTINGS_TOKEN) throw new Error('synthetic settings fixture is required');
const cache = path.join(home, '.fixture-vite-cache');
// 此进程是实际浏览器开发服务器，不是 Vitest 的 Node 模块执行器；复用生产 CJS 浏览器转换。
delete process.env.VITEST;
const reservation = createSocketServer();
await new Promise((resolve, reject) => { reservation.once('error', reject); reservation.listen(0, '127.0.0.1', resolve); });
const rendererPort = reservation.address().port;
await new Promise((resolve, reject) => reservation.close(error => error ? reject(error) : resolve()));
const server = await createServer({
  plugins: process.env.R06_SETTINGS_CASE === 'initialization' ? [{ name: 'r06-real-settings-reopen', transformIndexHtml: html => html.replace('src="./settings-main.tsx"', 'src="./react/settings/__tests__/r06-settings-reopen.tsx"') }] : [],
  configFile: path.join(repo, 'vite.config.ts'), root: path.join(repo, 'desktop/src'), cacheDir: cache,
  resolve: { alias: { '@': path.join(repo, 'desktop/src/react') } },
  esbuild: { jsx: 'automatic' },
  server: { host: '127.0.0.1', port: rendererPort, strictPort: true, fs: { allow: [repo] }, hmr: false },
  optimizeDeps: { include: ['react', 'react-dom/client', 'zustand'] },
  logLevel: 'error',
});
try {
  await server.listen();
  const address = server.httpServer.address();
  const executable = fs.realpathSync(path.join(repo, 'node_modules/electron/dist/Electron.app/Contents/MacOS/Electron'));
  if (!fs.existsSync(executable)) throw new Error('actual Electron executable is unavailable');
  const result = await new Promise((resolve, reject) => {
    const child = spawn(executable, [path.join(here, 'r06-settings-electron-main.cjs')], {
      cwd: home, env: { ...process.env, R06_SETTINGS_RENDERER_URL: `http://127.0.0.1:${address.port}/settings.html`, ELECTRON_ENABLE_LOGGING: '0' },
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    const timeout = setTimeout(() => { child.kill(); reject(new Error('real settings UI exceeded fixture timeout')); }, 120_000);
    child.stdout.on('data', chunk => process.stdout.write(chunk));
    child.stderr.on('data', chunk => process.stderr.write(chunk));
    child.on('error', reject);
    child.on('exit', (code, signal) => { clearTimeout(timeout); resolve({ code, signal }); });
  });
  if (result.code !== 0) throw new Error(`real Electron settings fixture failed: ${JSON.stringify(result)}`);
} finally { await server.close(); }
