// 外围 SDK 独立成包；不依赖旧服务或 Agent loop 进程。
import fs from 'node:fs';
import path from 'node:path';
import { buildSync } from 'esbuild';

export function buildBridgeSdkWorkers({ rootDir, bundleOutDir }) {
  fs.mkdirSync(bundleOutDir, { recursive: true });
  const artifacts = [];
  for (const name of ['bridge-sdk-worker', 'bridge-sdk-diagnostic']) {
    const outfile = path.join(bundleOutDir, `${name}.mjs`);
    const result = buildSync({
      absWorkingDir: rootDir,
      entryPoints: [path.join(rootDir, 'scripts', `${name}.ts`)],
      outfile, bundle: true, platform: 'node', target: 'node24', format: 'esm',
      packages: 'external', metafile: true, logLevel: 'silent',
    });
    if (Object.keys(result.metafile.inputs).some(file => /(?:^|\/)core\/(?:agent|engine|bridge-session-manager)/.test(file))) {
      throw new Error('Bridge SDK artifact unexpectedly includes the old execution loop');
    }
    artifacts.push(outfile);
  }
  return artifacts;
}
