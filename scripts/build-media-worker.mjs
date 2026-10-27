// 媒体外围包只复用成熟 SDK 的单操作纯函数；宿主持有全部执行、权限与网络。
import fs from 'node:fs';
import path from 'node:path';
import { buildSync } from 'esbuild';

export function buildMediaWorker({ rootDir, bundleOutDir }) {
  fs.mkdirSync(bundleOutDir, { recursive: true });
  const outfile = path.join(bundleOutDir, 'media-jimeng-worker.mjs');
  const result = buildSync({ absWorkingDir: rootDir, entryPoints: [path.join(rootDir, 'scripts/media-jimeng-worker.ts')], outfile, bundle: true, platform: 'node', target: 'node24', format: 'esm', packages: 'external', metafile: true, logLevel: 'silent' });
  const forbidden = Object.keys(result.metafile.inputs).filter(file => /(?:^|\/)core\/(?:agent|engine|media\/universal-media-manager|bridge-session-manager)(?:\.|\/)/.test(file));
  if (forbidden.length) throw new Error('Media worker unexpectedly includes an execution loop');
  return outfile;
}
