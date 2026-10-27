#!/usr/bin/env node
/**
 * scripts/tauri-build.mjs — R09 Tauri 宿主构建包装（T01 交付物）。
 *
 * 职责（设计 §3.3/§3.7/§3.8）：
 *   1. 记录并校验同一 React 产物 `desktop/dist-renderer`（树摘要）；
 *   2. release：校验 `dist-rust-service/<tag>-<arch>/build.json`（含 protocol 字段），
 *      把 sidecar stage 成 `desktop/src-tauri/binaries/lingxi-service-<triple>`，
 *      清单放 `desktop/src-tauri/resources/rust-service/build.json`；
 *   3. 从 package.json 注入 version，生成 overlay 配置（base `tauri.conf.json` 保持原样，
 *      R06 资源宿主 E2E 可复跑）；
 *   4. 调 `@tauri-apps/cli`（锁 2.12.4）：release 打 `.app`，debug 只出二进制
 *      （A01/A03-dev/A04 的 dev-preview 构建）；
 *   5. 写构建回执（`--receipt` 指向证据目录时同时写那里），含输入摘要/命令/产物/签名状态。
 *
 * 输入相同时默认复用上次成功产物（`--force` 强制重建）；缓存判定完全由回执摘要
 * 决定，不按 mtime 猜。所有命令真实执行，失败即非零退出。
 */
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { execFileSync, spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const TAURI_DIR = path.join(ROOT, 'desktop', 'src-tauri');
const BUILD_DIR = path.join(TAURI_DIR, '.tauri-build');
const TAURI_BIN = path.join(ROOT, 'node_modules', '.bin', 'tauri');

function sha256Bytes(bytes) {
  return crypto.createHash('sha256').update(bytes).digest('hex');
}

function sha256File(file) {
  return sha256Bytes(fs.readFileSync(file));
}

// 字节序（code-unit）排序：与生产者的 Python `sorted()`/`tree_digest` 及
// R08 基线树摘要完全一致；localeCompare 会把 `abnf…` 排在 `Animation…` 前，
// 得到与证据/回执互不相认的第二套摘要。
function byteOrder(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}

function sha256Tree(root) {
  const entries = [];
  const walk = (dir, relative) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => byteOrder(a.name, b.name))) {
      const name = relative ? `${relative}/${entry.name}` : entry.name;
      const full = path.join(dir, entry.name);
      if (entry.isSymbolicLink()) throw new Error(`renderer tree contains a symlink: ${name}`);
      if (entry.isDirectory()) walk(full, name);
      else if (entry.isFile()) entries.push([name, sha256File(full)]);
    }
  };
  walk(root, '');
  const hash = crypto.createHash('sha256');
  for (const [name, digest] of entries) {
    hash.update(name).update('\0').update(digest).update('\0');
  }
  return { digest: hash.digest('hex'), fileCount: entries.length };
}

function crateSourceDigest() {
  const skipDirs = new Set(['target', 'gen', 'binaries', 'resources', '.tauri-build']);
  const entries = [];
  const walk = (dir, relative) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => byteOrder(a.name, b.name))) {
      if (!relative && skipDirs.has(entry.name)) continue;
      const name = relative ? `${relative}/${entry.name}` : entry.name;
      const full = path.join(dir, entry.name);
      if (entry.isSymbolicLink()) throw new Error(`tauri source tree contains a symlink: ${name}`);
      if (entry.isDirectory()) walk(full, name);
      else if (entry.isFile()) entries.push([name, sha256File(full)]);
    }
  };
  walk(TAURI_DIR, '');
  const hash = crypto.createHash('sha256');
  for (const [name, digest] of entries) hash.update(name).update('\0').update(digest).update('\0');
  return { digest: hash.digest('hex'), fileCount: entries.length };
}

function buildRendererBundle(env) {
  // theme bundle 是 dist-renderer 的一部分（index.html 在 React 前加载
  // `lib/theme.js`）。缺它时 Electron file:// 只是安静 404，Tauri 自定义协议会
  // 回退 index.html → “Unexpected token '<'”。固定顺序 renderer → theme，
  // 不靠调用方记得手工跑两条命令。
  const commands = [
    ['run', 'build:renderer'],
    ['run', 'build:theme'],
  ];
  for (const args of commands) {
    execFileSync('npm', args, { cwd: ROOT, env, stdio: 'inherit' });
  }
  return commands.map((args) => ['npm', ...args].join(' '));
}

function readJson(file) {
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}

function packageVersion() {
  return readJson(path.join(ROOT, 'package.json')).version;
}

function rustupExecutable() {
  const candidates = [process.env.RUSTUP_BIN, path.join(os.homedir(), '.cargo', 'bin', 'rustup')];
  const binary = candidates.find((candidate) => candidate && fs.existsSync(candidate));
  if (!binary) throw new Error('tauri-build requires an absolute rustup executable (RUSTUP_BIN or ~/.cargo/bin/rustup)');
  return binary;
}

function cargoEnvironment() {
  const rustup = rustupExecutable();
  const cargoBin = path.dirname(rustup);
  return {
    ...process.env,
    PATH: `${cargoBin}${path.delimiter}${process.env.PATH || ''}`,
    CARGO: path.join(cargoBin, 'cargo'),
    RUSTUP_BIN: rustup,
  };
}

function hostTriple(env) {
  const output = execFileSync(rustupExecutable(), ['run', pinnedToolchain(), 'rustc', '-vV'], {
    cwd: ROOT,
    encoding: 'utf8',
    env,
  });
  const host = /\bhost:\s*(\S+)/.exec(output)?.[1];
  if (!host) throw new Error('rustc -vV did not report a host target');
  return host;
}

function pinnedToolchain() {
  const content = fs.readFileSync(path.join(ROOT, 'rust-toolchain.toml'), 'utf8');
  const channel = /^channel\s*=\s*"(\d+\.\d+\.\d+)"/m.exec(content)?.[1];
  if (!channel) throw new Error('rust-toolchain.toml has no pinned channel');
  return channel;
}

function tauriCliVersion() {
  return execFileSync(TAURI_BIN, ['--version'], { cwd: ROOT, encoding: 'utf8' }).trim();
}

function parseArgs(argv) {
  const options = {
    mode: 'release',
    force: false,
    buildService: false,
    receipt: null,
    bundles: null,
    features: [],
    rendererBuild: true,
    refresh: false,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--debug') options.mode = 'debug';
    else if (arg === '--release') options.mode = 'release';
    else if (arg === '--force') options.force = true;
    else if (arg === '--build-service') options.buildService = true;
    else if (arg === '--receipt') options.receipt = path.resolve(argv[++index]);
    else if (arg === '--bundles') options.bundles = argv[++index];
    else if (arg === '--no-bundle') options.bundles = null;
    else if (arg === '--features') options.features.push(...String(argv[++index]).split(','));
    else if (arg === '--no-renderer-build') options.rendererBuild = false;
    else if (arg === '--refresh') options.refresh = true;
    else throw new Error(`unknown tauri-build argument: ${arg}`);
  }
  if (options.mode === 'debug' && options.bundles === null) options.bundles = null;
  if (options.mode === 'release' && options.bundles === null) options.bundles = 'app';
  return options;
}

function resolveServiceStage() {
  const { stageDirectory, verifyStage } = require('../scripts/build-rust-desktop-service.mjs');
  const directory = stageDirectory(ROOT);
  if (!fs.existsSync(directory)) {
    throw new Error(`no prebuilt Rust service at ${directory}; pass --build-service or run npm run build:rust-service`);
  }
  return { directory, verified: verifyStage({ root: ROOT }) };
}

function stageSidecar(stage, rendererDigest, triple) {
  const binaries = path.join(TAURI_DIR, 'binaries');
  const resources = path.join(TAURI_DIR, 'resources', 'rust-service');
  fs.mkdirSync(binaries, { recursive: true });
  fs.mkdirSync(resources, { recursive: true });
  const sourceBinary = stage.verified.binary;
  const sidecar = path.join(binaries, `${path.basename(sourceBinary)}-${triple}`);
  fs.copyFileSync(sourceBinary, sidecar);
  if (process.platform !== 'win32') fs.chmodSync(sidecar, 0o755);
  const manifest = { ...readJson(path.join(stage.directory, 'build.json')) };
  manifest.rendererSha256 = rendererDigest;
  manifest.tauri = { cli: tauriCliVersion(), layout: 'externalBin+resources' };
  fs.writeFileSync(path.join(resources, 'build.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  const sidecarSha = sha256File(sidecar);
  return {
    sidecar: path.relative(ROOT, sidecar),
    sidecarSha256: sidecarSha,
    resourcesManifest: path.relative(ROOT, path.join(resources, 'build.json')),
    manifestSha256: sha256File(path.join(resources, 'build.json')),
    protocol: manifest.protocol,
    binarySha256: manifest.sha256,
    contentSha256: manifest.contentSha256,
  };
}

function cleanGeneratedPermissions() {
  // tauri-build 为 AppManifest 的每条命令生成 permissions/autogenerated/*.toml，
  // 但**不会删除**上一次构建遗留的旧文件。evidence 构建（r09-test-hooks）生成的
  // debug 命令权限若不清理，会进入后续交付 release 的 ACL（A12 攻击面泄漏）。
  const generated = path.join(TAURI_DIR, 'permissions', 'autogenerated');
  if (fs.existsSync(generated)) {
    for (const name of fs.readdirSync(generated)) {
      if (name.endsWith('.toml')) fs.rmSync(path.join(generated, name), { force: true });
    }
  }
  // 强制 build script 重跑（不清掉 target 缓存，只让 tauri-build 重新生成 ACL）。
  const buildScript = path.join(TAURI_DIR, 'build.rs');
  const now = new Date();
  fs.utimesSync(buildScript, now, now);
}

function stageTestHooksCapability(crateFeatures) {
  // R09-T06 D11：测试命令 capability 只在 evidence 构建 stage 进 capabilities/。
  // 交付 release 没有该文件，避免 tauri-build 因缺失 debug 命令权限而失败，
  // 也保证 release ACL 不含 debug 命令。
  const source = path.join(TAURI_DIR, 'capabilities-test', 'r09-test-hooks.json');
  const target = path.join(TAURI_DIR, 'capabilities', 'r09-test-hooks.json');
  if (crateFeatures.includes('r09-test-hooks')) {
    if (!fs.existsSync(source)) throw new Error(`test-hooks capability missing: ${source}`);
    fs.copyFileSync(source, target);
    return () => fs.rmSync(target, { force: true });
  }
  fs.rmSync(target, { force: true });
  return () => {};
}

function generateConfig(mode, version) {
  const templateFile = path.join(TAURI_DIR, 'tauri.release.conf.json');
  const config = readJson(templateFile);
  config.version = version;
  let target = path.join(TAURI_DIR, `tauri.${mode}.generated.conf.json`);
  if (mode === 'debug') {
    // dev-preview 构建：只出二进制（--no-bundle），不 stage sidecar/resources。
    config.bundle = { ...(config.bundle || {}), active: false, externalBin: [], resources: {} };
    delete config.bundle.macOS;
  }
  fs.mkdirSync(BUILD_DIR, { recursive: true });
  fs.writeFileSync(target, `${JSON.stringify(config, null, 2)}\n`);
  return {
    file: target,
    relative: path.relative(ROOT, target),
    templateSha256: sha256File(templateFile),
    productName: config.productName,
  };
}

function artifactPaths(mode, config) {
  if (mode === 'release') {
    const app = path.join(TAURI_DIR, 'target', 'release', 'bundle', 'macos', `${config.productName}.app`);
    return {
      appBundle: app,
      executable: path.join(app, 'Contents', 'MacOS', 'lingxi-resource-desktop'),
    };
  }
  return {
    appBundle: null,
    executable: path.join(TAURI_DIR, 'target', 'debug', 'lingxi-resource-desktop'),
  };
}

function codesignReport(appBundle) {
  if (!appBundle || !fs.existsSync(appBundle) || process.platform !== 'darwin') return null;
  const run = (args) => {
    const argv = ['/usr/bin/codesign', ...args, appBundle];
    const result = spawnSync(argv[0], argv.slice(1), {
      encoding: 'utf8', timeout: 60000, stdio: ['ignore', 'pipe', 'pipe'],
    });
    return {
      command: argv.join(' '),
      exitCode: typeof result.status === 'number' ? result.status : -1,
      stdout: String(result.stdout || '').trim(),
      stderr: String(result.stderr || (result.error ? result.error.message : '') || '').trim(),
    };
  };
  const details = run(['-dv', '--verbose=4']);
  const verify = run(['--verify', '--deep', '--strict']);
  // `codesign -dv --verbose=4` 将签名详情写 stderr（stdout 为空），两路合并解析。
  const detailText = [details.stdout, details.stderr].filter(Boolean).join('\n');
  const detailLines = detailText.split('\n').map((line) => line.trim()).filter(Boolean);
  const summaryLine = (prefix) => detailLines.find((line) => line.startsWith(prefix)) || null;
  return {
    verifyCommand: verify.command,
    verify,
    detailsCommand: details.command,
    details,
    summary: {
      identifier: summaryLine('Identifier='),
      signature: summaryLine('Signature='),
      format: summaryLine('Format='),
      teamIdentifier: summaryLine('TeamIdentifier='),
    },
  };
}

function refreshReceipt(options) {
  // 候选冻结用：候选产物已存在且不允许重建时，只重算元数据/签名/摘要并刷新回执。
  // 不执行 tauri build，不改变任何产物字节。
  const receiptPath = path.join(BUILD_DIR, `build-receipt-${options.mode}.json`);
  if (!fs.existsSync(receiptPath)) throw new Error(`refresh requires existing receipt: ${receiptPath}`);
  const previous = readJson(receiptPath);
  const config = generateConfig(options.mode, packageVersion());
  const crateFeatures = ['r09-host', ...options.features];
  const rendererRoot = path.join(ROOT, 'desktop', 'dist-renderer');
  const renderer = sha256Tree(rendererRoot);
  const theme = {
    path: 'lib/theme.js',
    sha256: sha256File(path.join(rendererRoot, 'lib', 'theme.js')),
    bytes: fs.statSync(path.join(rendererRoot, 'lib', 'theme.js')).size,
  };
  const drifted = previous.inputs.rendererDigest !== renderer.digest
    || previous.inputs.themeBundleSha256 !== theme.sha256
    || JSON.stringify(previous.inputs.features) !== JSON.stringify([...crateFeatures].sort());
  if (drifted) {
    throw new Error('refresh refused: renderer/theme/features differ from the frozen receipt inputs');
  }
  const artifacts = previous.artifacts || {};
  const executable = artifacts.executable;
  if (!executable || !fs.existsSync(executable)) throw new Error(`refresh requires executable: ${executable}`);
  const receipt = {
    ...previous,
    schema: previous.schema || 'lingxi.r09.tauri-build.v1',
    mode: options.mode,
    features: [...crateFeatures].sort(),
    evidenceBuild: crateFeatures.includes('r09-test-hooks'),
    refreshedAtUnixMs: Date.now(),
    cacheHit: true,
    command: null,
    config: { file: config.relative, generatedSha256: sha256File(config.file) },
    renderer: { digest: renderer.digest, fileCount: renderer.fileCount },
    themeBundle: theme,
    artifacts: {
      ...artifacts,
      executable,
      executableSha256: sha256File(executable),
      appBundle: artifacts.appBundle || null,
      appBundleExists: artifacts.appBundle ? fs.existsSync(artifacts.appBundle) : false,
    },
    codesign: codesignReport(artifacts.appBundle),
  };
  writeReceipt(receiptPath, receipt, options.receipt);
  console.log(`[tauri-build] refreshed receipt (no rebuild): ${receiptPath}`);
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.refresh) {
    refreshReceipt(options);
    return 0;
  }
  const env = cargoEnvironment();
  const version = packageVersion();
  const rendererBuildCommands = options.rendererBuild ? buildRendererBundle(env) : [];
  const rendererRoot = path.join(ROOT, 'desktop', 'dist-renderer');
  if (!fs.existsSync(path.join(rendererRoot, 'index.html'))) {
    throw new Error(`same React artifact missing: ${rendererRoot}/index.html (run npm run build:renderer)`);
  }
  // `index.html` 在 React 之前加载非模块 `lib/theme.js`（vite.config.theme.js 的
  // IIFE 产物写进 dist-renderer/lib/theme.js）。缺这一步时 Electron file:// 只是
  // 安静 404，Tauri 自定义协议会回退 index.html → “Unexpected token '<'”。
  const themeBundle = path.join(rendererRoot, 'lib', 'theme.js');
  if (!fs.existsSync(themeBundle)) {
    throw new Error(`theme bundle missing: ${themeBundle} (run npm run build:theme after build:renderer)`);
  }
  const theme = { path: 'lib/theme.js', sha256: sha256File(themeBundle), bytes: fs.statSync(themeBundle).size };
  const renderer = sha256Tree(rendererRoot);
  const triple = hostTriple(env);
  const cliVersion = tauriCliVersion();
  const config = generateConfig(options.mode, version);
  const crateFeatures = ['r09-host', ...options.features];

  let service = null;
  if (options.mode === 'release') {
    if (options.buildService) {
      const { buildStage } = require('../scripts/build-rust-desktop-service.mjs');
      buildStage({ root: ROOT });
    }
    const stage = resolveServiceStage();
    service = { stage: stage.directory, ...stageSidecar(stage, renderer.digest, triple) };
  }

  const source = crateSourceDigest();
  const inputs = {
    mode: options.mode,
    crateSourceDigest: source.digest,
    crateSourceFileCount: source.fileCount,
    rendererDigest: renderer.digest,
    rendererFileCount: renderer.fileCount,
    themeBundleSha256: theme.sha256,
    themeBundleBytes: theme.bytes,
    configTemplateSha256: config.templateSha256,
    packageVersion: version,
    tauriCli: cliVersion,
    toolchain: pinnedToolchain(),
    target: triple,
    serviceManifestSha256: service?.manifestSha256 ?? null,
    serviceBuildJsonSha256: service ? sha256File(path.join(service.stage, 'build.json')) : null,
    // r09-host 是产品/验收构建的必需 feature（启用服务 supervisor + 产品窗口）；
    // 不允许 A01/A02/A14 引用无 feature 的 R06_COMPAT_ONLY 构建。
    features: [...crateFeatures].sort(),
    evidenceBuild: crateFeatures.includes('r09-test-hooks'),
    hostFeature: 'r09-host',
    hostFeatureEffects: ['service-supervisor', 'product-react-window', 'host-boot-commands'],
  };
  const receiptPath = path.join(BUILD_DIR, `build-receipt-${options.mode}.json`);
  if (!options.force && fs.existsSync(receiptPath)) {
    try {
      const previous = readJson(receiptPath);
      const sameInputs = JSON.stringify(previous.inputs) === JSON.stringify(inputs);
      // 产物完整性：别的构建（例如 R06 `cargo build` 用 base config）可能覆盖
      // target/debug 二进制；缓存命中必须核对回执记录的文件 sha，而不是“文件存在”。
      const executable = previous.artifacts?.executable;
      const artifactOk = Boolean(
        executable
        && fs.existsSync(executable)
        && sha256File(executable) === previous.artifacts?.executableSha256
        && (!previous.artifacts?.appBundle
          || (fs.existsSync(previous.artifacts.appBundle)
            && fs.existsSync(previous.artifacts.executable))),
      );
      if (sameInputs && artifactOk) {
        console.log(`[tauri-build] cache hit (${options.mode}); artifact ${previous.artifacts.executable}`);
        stageTestHooksCapability(crateFeatures)();
        writeReceipt(receiptPath, { ...previous, cacheHit: true, command: null, rendererBuild: rendererBuildCommands, evidenceBuild: crateFeatures.includes('r09-test-hooks') }, options.receipt);
        return 0;
      }
    } catch (error) {
      console.warn(`[tauri-build] ignoring unreadable previous receipt: ${error.message}`);
    }
    console.log(`[tauri-build] cache invalid for ${options.mode}: inputs or artifact identity changed; rebuilding`);
  }

  const argv = ['build', '--config', config.file, '--features', crateFeatures.join(',')];
  if (options.mode === 'debug') argv.push('--debug', '--no-bundle');
  else if (options.bundles) argv.push('--bundles', options.bundles);
  console.log(`[tauri-build] $ tauri ${argv.join(' ')}`);
  cleanGeneratedPermissions();
  const cleanupTestHooksCapability = stageTestHooksCapability(crateFeatures);
  try {
    execFileSync(TAURI_BIN, argv, { cwd: ROOT, env, stdio: 'inherit' });
  } finally {
    cleanupTestHooksCapability();
  }

  const paths = artifactPaths(options.mode, config);
  if (!fs.existsSync(paths.executable)) {
    throw new Error(`tauri build finished but expected executable is missing: ${paths.executable}`);
  }
  const receipt = {
    schema: 'lingxi.r09.tauri-build.v1',
    mode: options.mode,
    features: [...crateFeatures].sort(),
    evidenceBuild: crateFeatures.includes('r09-test-hooks'),
    builtAtUnixMs: Date.now(),
    cacheHit: false,
    command: ['tauri', ...argv],
    rendererBuild: rendererBuildCommands,
    inputs,
    config: { file: config.relative, generatedSha256: sha256File(config.file) },
    themeBundle: theme,
    artifacts: {
      executable: paths.executable,
      executableSha256: sha256File(paths.executable),
      appBundle: paths.appBundle,
      appBundleExists: paths.appBundle ? fs.existsSync(paths.appBundle) : false,
    },
    service,
    codesign: codesignReport(paths.appBundle),
  };
  writeReceipt(receiptPath, receipt, options.receipt);
  console.log(`[tauri-build] receipt ${receiptPath}`);
  return 0;
}

function writeReceipt(receiptPath, receipt, extraPath) {
  fs.mkdirSync(path.dirname(receiptPath), { recursive: true });
  const text = `${JSON.stringify(receipt, null, 2)}\n`;
  fs.writeFileSync(receiptPath, text);
  if (extraPath && path.resolve(extraPath) !== path.resolve(receiptPath)) {
    fs.mkdirSync(path.dirname(extraPath), { recursive: true });
    fs.writeFileSync(extraPath, text);
  }
}

try {
  process.exitCode = main();
} catch (error) {
  console.error(`[tauri-build] ${error?.stack || error}`);
  process.exitCode = 1;
}
