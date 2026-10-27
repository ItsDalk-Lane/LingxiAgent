#!/usr/bin/env node
/**
 * scripts/tauri-dev.mjs — R09 Tauri 开发宿主启动器（T01 交付物）。
 *
 * 与 Electron `npm run start:vite` 的边界一致：Vite 只服务开发页面，
 * **不进入 release 产物**（A02 的 release 路径由 scripts/tauri-build.mjs 保证）。
 *
 * 流程：起 Vite(5173，`npm run dev:renderer`) → 等端口可连 → 起
 * `tauri dev --config tauri.dev.conf.json`（withGlobalTauri + devUrl）→
 * 任一退出即连带清理另一子进程。
 *
 * 注意：开发模式的服务二进制走 `LINGXI_SERVICE_BIN`（debug/dev-preview 语义，
 * 与 release 的 sidecar+build.json 校验分离；见 docs/rust-tauri/R09/
 * T01_T02_HOST_SERVICE_DESIGN.md §4.4）。
 */
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import net from 'node:net';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const TAURI_DIR = path.join(ROOT, 'desktop', 'src-tauri');
const TAURI_BIN = path.join(ROOT, 'node_modules', '.bin', 'tauri');
const VITE_PORT = Number(process.env.LINGXI_DEV_PORT || 5173);

function cargoEnvironment() {
  const rustup = process.env.RUSTUP_BIN
    || path.join(os.homedir(), '.cargo', 'bin', 'rustup');
  if (!fs.existsSync(rustup)) throw new Error('tauri-dev requires rustup (RUSTUP_BIN or ~/.cargo/bin/rustup)');
  const cargoBin = path.dirname(rustup);
  return {
    ...process.env,
    PATH: `${cargoBin}${path.delimiter}${process.env.PATH || ''}`,
    CARGO: path.join(cargoBin, 'cargo'),
    RUSTUP_BIN: rustup,
  };
}

function waitForPort(port, timeoutMs = 60000) {
  const started = Date.now();
  return new Promise((resolve, reject) => {
    const attempt = () => {
      const socket = net.connect({ host: '127.0.0.1', port });
      socket.once('connect', () => {
        socket.destroy();
        resolve();
      });
      socket.once('error', () => {
        socket.destroy();
        if (Date.now() - started > timeoutMs) reject(new Error(`vite dev server did not listen on ${port} within ${timeoutMs}ms`));
        else setTimeout(attempt, 250);
      });
    };
    attempt();
  });
}

async function main() {
  const env = cargoEnvironment();
  const config = path.join(TAURI_DIR, 'tauri.dev.conf.json');
  if (!fs.existsSync(config)) throw new Error(`missing dev overlay: ${config}`);
  const children = new Set();
  const shutdown = (code) => {
    for (const child of children) {
      try { child.kill('SIGTERM'); } catch {}
    }
    process.exitCode = code;
  };

  const vite = spawn('npm', ['run', 'dev:renderer'], { cwd: ROOT, env, stdio: 'inherit' });
  children.add(vite);
  await waitForPort(VITE_PORT);
  console.log(`[tauri-dev] vite ready on http://localhost:${VITE_PORT}; starting tauri dev`);
  const tauri = spawn(TAURI_BIN, ['dev', '--config', config, '--features', 'r09-host'], {
    cwd: ROOT,
    env: { ...env, LINGXI_TAURI_DEV: '1' },
    stdio: 'inherit',
  });
  children.add(tauri);
  let exiting = false;
  const onExit = (source) => (code, signal) => {
    if (exiting) return;
    exiting = true;
    console.log(`[tauri-dev] ${source} exited (code=${code} signal=${signal}); shutting down`);
    for (const child of children) {
      if (child === source) continue;
      try { child.kill('SIGTERM'); } catch {}
    }
    shutdown(code ?? 0);
  };
  vite.on('exit', onExit(vite));
  tauri.on('exit', onExit(tauri));
  process.on('SIGINT', () => shutdown(130));
  process.on('SIGTERM', () => shutdown(143));
}

main().catch((error) => {
  console.error(`[tauri-dev] ${error?.stack || error}`);
  process.exitCode = 1;
});
