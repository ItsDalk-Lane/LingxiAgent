const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');
const {
  rustDesktopEnabled,
  rustServicePlatformTag,
  rustServiceExecutable,
  rustServiceContentDigest,
  verifyRustServiceDirectory,
  resolveRustBinary,
  readOwnedRustConnection,
  readExistingRustConnection,
} = require('../desktop/src/shared/rust-local-service.cjs');
const { createPrivateServerArtifactGuard } = require('../desktop/src/shared/private-server-artifact.cjs');

test('Windows 产物守卫只调用受信安装资源程序且拒绝失败', async () => {
  const calls = [];
  const guard = createPrivateServerArtifactGuard({
    resourcesPath: '/trusted/resources', appVersion: '0.1.43', platform: 'win32', electron: true,
    selectBinary: (options) => {
      assert.deepEqual(options, { packaged: true, resourcesPath: '/trusted/resources', appVersion: '0.1.43' });
      return '/trusted/resources/rust-service/lingxi-service.exe';
    },
    run: (binary, args) => calls.push({ binary, args }),
  });
  await guard('prepare', '/private/home');
  await guard('seal', '/private/home', '/private/home/artifacts/server/new.tmp');
  await guard('verify', '/private/home', '/private/home/artifacts/server/current');
  assert.deepEqual(calls.map((call) => call.args[0]), [
    '--prepare-private-artifacts', '--seal-private-artifact-tree', '--verify-private-artifact-tree',
  ]);
  assert.equal(calls.every((call) => call.binary === '/trusted/resources/rust-service/lingxi-service.exe'), true);
  const broken = createPrivateServerArtifactGuard({
    resourcesPath: '/trusted/resources', appVersion: '0.1.43', platform: 'win32', electron: true,
    selectBinary: () => '/trusted/reader.exe', run: () => { throw new Error('denied'); },
  });
  await assert.rejects(broken('verify', '/private/home', '/private/home/artifacts/server/current'), /refusing to trust/);
  assert.equal(createPrivateServerArtifactGuard({ platform: 'darwin', electron: true }), null);
});

test('桌面 Rust 模式必须显式选择，未知值拒绝', () => {
  assert.equal(rustDesktopEnabled({}), false);
  assert.equal(rustDesktopEnabled({ LINGXI_DESKTOP_SERVER_RUNTIME: 'rust' }), true);
  assert.throws(() => rustDesktopEnabled({ LINGXI_DESKTOP_SERVER_RUNTIME: 'other' }), /invalid/);
});

function stagedBinary(t, { platform = process.platform, arch = process.arch } = {}) {
  const resourcesPath = fs.mkdtempSync(path.join(os.tmpdir(), 'lingxi-rust-package-'));
  t.after(() => fs.rmSync(resourcesPath, { recursive: true, force: true }));
  const directory = path.join(resourcesPath, 'rust-service');
  fs.mkdirSync(directory);
  const binary = path.join(directory, rustServiceExecutable(platform));
  const bytes = Buffer.alloc(2048);
  if (platform === 'darwin') {
    bytes.writeUInt32LE(0xfeedfacf, 0);
    bytes.writeUInt32LE(arch === 'arm64' ? 0x0100000c : 0x01000007, 4);
  } else if (platform === 'linux') {
    Buffer.from('7f454c460201', 'hex').copy(bytes);
    bytes.writeUInt16LE(arch === 'arm64' ? 183 : 62, 18);
  } else {
    bytes.write('MZ', 0);
    bytes.writeUInt32LE(128, 0x3c);
    bytes.write('PE\0\0', 128);
    bytes.writeUInt16LE(arch === 'arm64' ? 0xaa64 : 0x8664, 132);
    bytes.writeUInt16LE(1, 134);
    bytes.writeUInt16LE(240, 148);
    bytes.writeUInt16LE(0x20b, 152);
    bytes.writeUInt32LE(1536, 408);
    bytes.writeUInt32LE(512, 412);
  }
  fs.writeFileSync(binary, bytes, { mode: 0o755 });
  const manifest = {
    schemaVersion: 1, platform: rustServicePlatformTag(platform), arch,
    target: 'fixture-target', toolchain: '1.98.1', appVersion: '0.1.43',
    binary: path.basename(binary),
    sha256: crypto.createHash('sha256').update(bytes).digest('hex'),
    contentSha256: rustServiceContentDigest(bytes, platform),
    sourceSha256: 'a'.repeat(64),
  };
  const manifestPath = path.join(directory, 'build.json');
  fs.writeFileSync(manifestPath, JSON.stringify(manifest));
  return { resourcesPath, directory, binary, manifest, manifestPath };
}

test('打包 Rust 模式从本机资源定位程序，环境覆盖无效', (t) => {
  const fixture = stagedBinary(t);
  assert.equal(resolveRustBinary({ root: '/tmp/unused', packaged: true,
    resourcesPath: fixture.resourcesPath, appVersion: '0.1.43',
    env: { LINGXI_SERVICE_BIN: '/tmp/forged' } }), fixture.binary);
  assert.throws(() => resolveRustBinary({ root: '/tmp/unused', packaged: true,
    resourcesPath: fixture.resourcesPath, appVersion: '0.1.44', env: {} }), /does not match/);
  assert.equal(verifyRustServiceDirectory({ directory: fixture.directory }).manifest.sha256, fixture.manifest.sha256);
});

test('打包 Rust 程序缺失、错平台、损坏或被换成链接时拒绝', (t) => {
  const fixture = stagedBinary(t);
  fs.writeFileSync(fixture.binary, Buffer.alloc(2048), { mode: 0o755 });
  assert.throws(() => verifyRustServiceDirectory({ directory: fixture.directory }), /matching .* executable/);
  const correct = stagedBinary(t);
  correct.manifest.arch = correct.manifest.arch === 'x64' ? 'arm64' : 'x64';
  fs.writeFileSync(correct.manifestPath, JSON.stringify(correct.manifest));
  assert.throws(() => verifyRustServiceDirectory({ directory: correct.directory }), /does not match/);
  correct.manifest.arch = process.arch;
  fs.writeFileSync(correct.manifestPath, JSON.stringify(correct.manifest));
  fs.appendFileSync(correct.binary, 'tamper');
  assert.throws(() => verifyRustServiceDirectory({ directory: correct.directory }), /digest mismatch/);
  fs.rmSync(correct.binary);
  fs.symlinkSync(fixture.binary, correct.binary);
  assert.throws(() => verifyRustServiceDirectory({ directory: correct.directory }), /regular executable/);
});

test('打包配置四个发行入口均准备同平台 Rust 程序，并纳入装箱与签名', () => {
  const pkg = JSON.parse(fs.readFileSync(path.join(__dirname, '..', 'package.json'), 'utf8'));
  for (const name of ['pack', 'dist', 'dist:win', 'dist:linux']) {
    assert.match(pkg.scripts[name], /npm run build:rust-service/);
    assert.match(pkg.scripts[name], /npm run verify:seed-kit/);
  }
  assert.match(pkg.scripts['dist:win'], /npm run build:rust-service && npm run build:server && npm run verify:seed-kit/);
  assert.ok(pkg.build.extraResources.some((entry) => entry.from === 'dist-rust-service/${os}-${arch}/' && entry.to === 'rust-service/'));
  assert.ok(pkg.build.mac.binaries.includes('Contents/Resources/rust-service/lingxi-service'));
  assert.match(fs.readFileSync(path.join(__dirname, '..', 'scripts/build-shell.mjs'), 'utf8'), /verifyRustServiceStage/);
  assert.match(fs.readFileSync(path.join(__dirname, '..', 'scripts/fix-modules.cjs'), 'utf8'), /packagedRust\.manifest\.contentSha256/);
});

test('Windows 签名只允许改 PE 校验和与证书表，程序正文改变则拒绝', (t) => {
  const fixture = stagedBinary(t, { platform: 'win32', arch: 'x64' });
  const before = fs.readFileSync(fixture.binary);
  const signed = Buffer.concat([before, Buffer.alloc(24, 0x5a)]);
  signed.writeUInt32LE(0x12345678, 152 + 64);
  signed.writeUInt32LE(before.length, 152 + 144);
  signed.writeUInt32LE(24, 152 + 148);
  fs.writeFileSync(fixture.binary, signed);
  assert.equal(rustServiceContentDigest(signed, 'win32'), fixture.manifest.contentSha256);
  assert.throws(() => verifyRustServiceDirectory({ directory: fixture.directory, platform: 'win32', arch: 'x64' }), /digest mismatch/);
  assert.equal(verifyRustServiceDirectory({
    directory: fixture.directory, platform: 'win32', arch: 'x64', allowSignedWindowsMutation: true,
  }).binary, fixture.binary);
  signed[500] ^= 1;
  fs.writeFileSync(fixture.binary, signed);
  assert.throws(() => verifyRustServiceDirectory({
    directory: fixture.directory, platform: 'win32', arch: 'x64', allowSignedWindowsMutation: true,
  }), /digest mismatch/);
});

test('macOS 签名只允许改 Mach-O 签名块和对应长度，正文改变则拒绝', () => {
  const unsigned = Buffer.alloc(2048);
  unsigned.writeUInt32LE(0xfeedfacf, 0);
  unsigned.writeUInt32LE(0x0100000c, 4);
  unsigned.writeUInt32LE(2, 16);
  unsigned.writeUInt32LE(88, 20);
  unsigned.writeUInt32LE(0x19, 32);
  unsigned.writeUInt32LE(72, 36);
  unsigned.write('__LINKEDIT', 40);
  unsigned.writeBigUInt64LE(4096n, 64);
  unsigned.writeBigUInt64LE(512n, 72);
  unsigned.writeBigUInt64LE(1536n, 80);
  unsigned.writeUInt32LE(0x1d, 104);
  unsigned.writeUInt32LE(16, 108);
  unsigned.writeUInt32LE(1536, 112);
  unsigned.writeUInt32LE(512, 116);
  unsigned.fill(0x31, 512, 1536);
  unsigned.fill(0x41, 1536);
  const expected = rustServiceContentDigest(unsigned, 'darwin');

  const signed = Buffer.from(unsigned.subarray(0, 1792));
  signed.writeBigUInt64LE(1280n, 80);
  signed.writeUInt32LE(256, 116);
  signed.fill(0x53, 1536);
  assert.equal(rustServiceContentDigest(signed, 'darwin'), expected);
  signed[600] ^= 1;
  assert.notEqual(rustServiceContentDigest(signed, 'darwin'), expected);
});

test('预构建 Rust 程序跟随固定工具链、全部源码输入和应用版本变化失效', async (t) => {
  const { pinnedToolchain, rustSourceDigest, verifyStage } = await import('../scripts/build-rust-desktop-service.mjs');
  assert.equal(pinnedToolchain(), '1.98.1');
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'lingxi-rust-stage-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.mkdirSync(path.join(root, 'rust', 'crates'), { recursive: true });
  fs.writeFileSync(path.join(root, 'rust-toolchain.toml'), '[toolchain]\nchannel = "1.98.1"\n');
  fs.writeFileSync(path.join(root, 'package.json'), JSON.stringify({ version: '0.1.43' }));
  fs.writeFileSync(path.join(root, 'rust', 'Cargo.toml'), '[workspace]\n');
  fs.writeFileSync(path.join(root, 'rust', 'crates', 'schema.json'), '{"v":1}\n');
  const fixture = stagedBinary(t);
  const directory = path.join(root, 'dist-rust-service', `${rustServicePlatformTag()}-${process.arch}`);
  fs.mkdirSync(path.dirname(directory), { recursive: true });
  fs.cpSync(fixture.directory, directory, { recursive: true });
  const manifestPath = path.join(directory, 'build.json');
  const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
  manifest.sourceSha256 = rustSourceDigest(path.join(root, 'rust'));
  fs.writeFileSync(manifestPath, JSON.stringify(manifest));
  assert.equal(verifyStage({ root }).binary, path.join(directory, rustServiceExecutable()));
  fs.writeFileSync(path.join(root, 'rust', 'crates', 'schema.json'), '{"v":2}\n');
  assert.throws(() => verifyStage({ root }), /stale/);
  fs.writeFileSync(path.join(root, 'rust', 'crates', 'schema.json'), '{"v":1}\n');
  fs.writeFileSync(path.join(root, 'package.json'), JSON.stringify({ version: '0.1.44' }));
  assert.throws(() => verifyStage({ root }), /stale/);
});

test('只接受本轮进程、数据根、READY 地址和短令牌一致的 Rust 记录', (t) => {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'lingxi-desktop-rust-'));
  t.after(() => fs.rmSync(home, { recursive: true, force: true }));
  const runtime = path.join(home, 'lingxi-service');
  fs.mkdirSync(runtime);
  const instance = {
    serverKind: 'lingxi-service', instanceId: 'a'.repeat(32), pid: 1254,
    homePath: fs.realpathSync(home), bindAddr: '127.0.0.1:34001', transport: 'https',
  };
  const token = { kind: 'local_token', instanceId: instance.instanceId, token: 'b'.repeat(32) };
  const write = () => {
    fs.writeFileSync(path.join(runtime, 'instance.json'), JSON.stringify(instance), { mode: 0o600 });
    fs.writeFileSync(path.join(runtime, 'local-token.json'), JSON.stringify(token), { mode: 0o600 });
  };
  write();
  assert.deepEqual(readOwnedRustConnection({ home, pid: 1254, readyAddr: instance.bindAddr }), {
    port: 34001, transport: 'https', baseUrl: 'https://127.0.0.1:34001',
    token: token.token, instanceId: instance.instanceId,
  });
  assert.throws(() => readOwnedRustConnection({ home, pid: 1255, readyAddr: instance.bindAddr }), /mismatch/);
  assert.throws(() => readOwnedRustConnection({ home, pid: 1254, readyAddr: '127.0.0.1:34002' }), /mismatch/);
  token.instanceId = 'c'.repeat(32);
  write();
  assert.throws(() => readOwnedRustConnection({ home, pid: 1254, readyAddr: instance.bindAddr }), /mismatch/);
  token.instanceId = instance.instanceId;
  instance.bindAddr = '0.0.0.0:34001';
  write();
  assert.throws(() => readOwnedRustConnection({ home, pid: 1254, readyAddr: instance.bindAddr }), /local address/);
  instance.bindAddr = '127.0.0.1:34001';
  instance.pid = process.pid;
  write();
  assert.equal(readExistingRustConnection({ home }).pid, process.pid);
});
