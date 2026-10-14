// RV5 rerun of W12-R01-F02 (v2): schedule the second-connection upgrade INSIDE the usage-refresh
// transaction, right after assertWritableSchema passes — the exact window the original bug hit.
import path from 'node:path';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
const root = '/tmp/lingxi-rv5';
const { PortableVectorIndexAdapter } = await import(root + '/lib/knowledge/vector-index-adapter.ts');
const require = createRequire(root + '/package.json');
const Database = require('better-sqlite3');
const dbPath = path.join(process.env.LINGXI_HOME, 'query-race-v2.db');
let now = '2020-01-01T00:00:00Z';
const old = new PortableVectorIndexAdapter({ dbPath, now: () => now });
const model = { key: 'model', provider: 'local', modelId: 'synthetic', protocol: 'test', dimensions: 2 };
old.buildOrReplaceArtifact({ parseArtifactId: 'artifact', chunkFingerprint: 'fingerprint', model, entries: [{ chunkId: 'chunk', parseArtifactId: 'artifact', ordinal: 0, vector: [0.2, 0.8] }] });
const newer = new Database(dbPath);
newer.pragma('busy_timeout = 0');
const before = newer.prepare('SELECT last_used_at FROM vector_index_variants').get();

let crossed = false, upgradeError = null;
// Hook the version assert used by every write path: fire the second-connection upgrade at the
// moment the old handle has just validated the schema and is about to write.
const origAssert = Object.getPrototypeOf(old).assertWritableSchema;
old.assertWritableSchema = function (...args) {
  if (!crossed) {
    crossed = true;
    try { newer.pragma('user_version=999'); } catch (e) { upgradeError = { code: e.code, message: e.message }; }
  }
  return origAssert.apply(this, args);
};
// Belt-and-braces: also keep the original prepare hook in case the UPDATE bypasses the tx wrapper.
const prepare = old.db.prepare.bind(old.db);
old.db.prepare = (sql) => {
  if (sql.includes('UPDATE vector_index_variants SET last_used_at') && !crossed) {
    crossed = true;
    try { newer.pragma('user_version=999'); } catch (e) { upgradeError = { code: e.code, message: e.message }; }
  }
  return prepare(sql);
};

now = '2040-01-01T00:00:00Z';
let result, error;
try { result = old.search({ parseArtifactIds: ['artifact'], model, queryVector: [0.2, 0.8] }); }
catch (e) { error = { code: e.code, message: e.message, cause: e.cause && String(e.cause) }; }
const after = newer.prepare('SELECT last_used_at FROM vector_index_variants').get();
const version = newer.pragma('user_version', { simple: true });
console.log(JSON.stringify({ home: process.env.LINGXI_HOME, crossed, upgradeError, before, after, version, result: result ? '<hits>' : undefined, error }, null, 1));
newer.close(); old.close();
// The fix property: the old handle must never leave a future-version DB with its own usage write.
assert.equal(crossed, true, 'upgrade window must be exercised');
assert.ok(!(version === 999 && JSON.stringify(after) !== JSON.stringify(before)),
  '旧句柄在升级后不得改写future库的last_used_at');
console.log('RV5-F02-RERUN: PASS');
