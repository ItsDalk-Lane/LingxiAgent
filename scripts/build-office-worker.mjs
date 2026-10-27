// 文档外围包只包含成熟解析器入口，不装载旧服务或 Agent loop。
import fs from 'node:fs';
import path from 'node:path';
import { buildSync } from 'esbuild';

export function buildOfficeWorker({ rootDir, bundleOutDir }) {
  fs.mkdirSync(bundleOutDir, { recursive: true });
  const outfile = path.join(bundleOutDir, 'office-document-worker.mjs');
  const result = buildSync({ absWorkingDir: rootDir, entryPoints: [path.join(rootDir, 'scripts/office-document-worker.ts')], outfile, bundle: true, platform: 'node', target: 'node24', format: 'esm', packages: 'external', metafile: true, logLevel: 'silent' });
  if (Object.keys(result.metafile.inputs).some(file => /(?:^|\/)core\//.test(file))) throw new Error('Office worker unexpectedly includes core execution code');
  return outfile;
}
