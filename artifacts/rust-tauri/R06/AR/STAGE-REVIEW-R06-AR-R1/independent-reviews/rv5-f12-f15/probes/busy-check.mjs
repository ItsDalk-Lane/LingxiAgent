// Diagnostic: does a plain second connection cause the usage-refresh BUSY?
import path from 'node:path';
import { createRequire } from 'node:module';
const root = '/tmp/lingxi-rv5';
const { PortableVectorIndexAdapter } = await import(root + '/lib/knowledge/vector-index-adapter.ts');
const require = createRequire(root + '/package.json');
const Database = require('better-sqlite3');
const dbPath = path.join(process.env.LINGXI_HOME, 'busy-check.db');
let now = '2020-01-01T00:00:00Z';
const old = new PortableVectorIndexAdapter({ dbPath, now: () => now });
const model = { key: 'model', provider: 'local', modelId: 'synthetic', protocol: 'test', dimensions: 2 };
old.buildOrReplaceArtifact({ parseArtifactId: 'artifact', chunkFingerprint: 'fingerprint', model, entries: [{ chunkId: 'chunk', parseArtifactId: 'artifact', ordinal: 0, vector: [0.2, 0.8] }] });
const newer = new Database(dbPath);
const before = newer.prepare('SELECT last_used_at FROM vector_index_variants').get();
now = '2040-01-01T00:00:00Z';
const result = old.search({ parseArtifactIds: ['artifact'], model, queryVector: [0.2, 0.8] });
const after = newer.prepare('SELECT last_used_at FROM vector_index_variants').get();
console.log(JSON.stringify({ before, after, refreshed: after.last_used_at === '2040-01-01T00:00:00Z', journal: newer.pragma('journal_mode', { simple: true }) }));
newer.close(); old.close();
