const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { execFileSync } = require('child_process');

function rustServicePlatformTag(platform = process.platform) {
  if (platform === 'darwin') return 'mac';
  if (platform === 'win32') return 'win';
  if (platform === 'linux') return 'linux';
  throw new Error(`unsupported Rust desktop platform: ${platform}`);
}

function rustServiceExecutable(platform = process.platform) {
  return platform === 'win32' ? 'lingxi-service.exe' : 'lingxi-service';
}

function assertRustExecutableFormat(bytes, platform, arch) {
  const header = bytes.subarray(0, 256);
  if (platform === 'darwin') {
    const cpu = arch === 'arm64' ? 0x0100000c : 0x01000007;
    if (header.readUInt32LE(0) !== 0xfeedfacf || header.readUInt32LE(4) !== cpu) {
      throw new Error('Rust service is not a matching macOS executable');
    }
  } else if (platform === 'linux') {
    const machine = arch === 'arm64' ? 183 : 62;
    if (header.subarray(0, 4).toString('hex') !== '7f454c46'
        || header[4] !== 2 || header[5] !== 1 || header.readUInt16LE(18) !== machine) {
      throw new Error('Rust service is not a matching Linux executable');
    }
  } else if (platform === 'win32') {
    const peOffset = header.readUInt32LE(0x3c);
    const pe = bytes.subarray(peOffset, peOffset + 6);
    const machine = arch === 'arm64' ? 0xaa64 : 0x8664;
    if (header.subarray(0, 2).toString() !== 'MZ'
        || pe.length !== 6
        || pe.subarray(0, 4).toString('hex') !== '50450000'
        || pe.readUInt16LE(4) !== machine) {
      throw new Error('Rust service is not a matching Windows executable');
    }
  }
}

function macExecutableContentDigest(bytes) {
  if (bytes.length < 32 || bytes.readUInt32LE(0) !== 0xfeedfacf) {
    throw new Error('invalid macOS Rust executable');
  }
  const commands = bytes.readUInt32LE(16);
  const commandsEnd = 32 + bytes.readUInt32LE(20);
  if (commands > 512 || commandsEnd > bytes.length) throw new Error('invalid Mach-O load commands');
  if (commands === 0) return crypto.createHash('sha256').update(bytes).digest('hex');
  let offset = 32;
  let signature = null;
  let linkedit = null;
  for (let index = 0; index < commands; index++) {
    if (offset + 8 > commandsEnd) throw new Error('truncated Mach-O load command');
    const command = bytes.readUInt32LE(offset);
    const size = bytes.readUInt32LE(offset + 4);
    if (size < 8 || offset + size > commandsEnd) throw new Error('invalid Mach-O load command size');
    if (command === 0x1d) {
      if (signature || size < 16) throw new Error('invalid Mach-O code signature command');
      signature = { offset, dataOffset: bytes.readUInt32LE(offset + 8), size: bytes.readUInt32LE(offset + 12) };
    }
    if (command === 0x19 && bytes.subarray(offset + 8, offset + 24).toString('utf8').replace(/\0.*$/, '') === '__LINKEDIT') {
      if (linkedit || size < 72) throw new Error('invalid Mach-O linkedit segment');
      linkedit = { offset, fileOffset: Number(bytes.readBigUInt64LE(offset + 40)), fileSize: Number(bytes.readBigUInt64LE(offset + 48)) };
    }
    offset += size;
  }
  if (offset !== commandsEnd) throw new Error('Mach-O load command extent mismatch');
  if (!signature) return crypto.createHash('sha256').update(bytes).digest('hex');
  if (!linkedit || signature.dataOffset < commandsEnd || signature.size < 1
      || signature.dataOffset + signature.size !== bytes.length
      || linkedit.fileOffset > signature.dataOffset
      || linkedit.fileOffset + linkedit.fileSize !== bytes.length) {
    throw new Error('invalid Mach-O code signature extent');
  }
  // 签名可重写签名块及其三处长度字段；可执行正文、载入命令和其余字节仍须逐字相同。
  const content = Buffer.from(bytes.subarray(0, signature.dataOffset));
  content.writeBigUInt64LE(0n, linkedit.offset + 32);
  content.writeBigUInt64LE(0n, linkedit.offset + 48);
  content.writeUInt32LE(0, signature.offset + 12);
  return crypto.createHash('sha256').update(content).digest('hex');
}

function rustServiceContentDigest(bytes, platform = process.platform) {
  if (platform === 'darwin') return macExecutableContentDigest(bytes);
  if (platform !== 'win32') return crypto.createHash('sha256').update(bytes).digest('hex');
  const peOffset = bytes.readUInt32LE(0x3c);
  const optional = peOffset + 24;
  const magic = bytes.readUInt16LE(optional);
  const security = optional + (magic === 0x20b ? 112 : magic === 0x10b ? 96 : -1000) + 32;
  const checksum = optional + 64;
  if (security < checksum + 4 || security + 8 > bytes.length) throw new Error('invalid PE certificate directory');
  const certOffset = bytes.readUInt32LE(security);
  const certSize = bytes.readUInt32LE(security + 4);
  const sectionCount = bytes.readUInt16LE(peOffset + 6);
  const sectionTable = optional + bytes.readUInt16LE(peOffset + 20);
  if (sectionCount < 1 || sectionCount > 96 || sectionTable + sectionCount * 40 > bytes.length) {
    throw new Error('invalid PE section table');
  }
  let sectionEnd = sectionTable + sectionCount * 40;
  for (let index = 0; index < sectionCount; index++) {
    const entry = sectionTable + index * 40;
    sectionEnd = Math.max(sectionEnd, bytes.readUInt32LE(entry + 20) + bytes.readUInt32LE(entry + 16));
  }
  if ((certOffset === 0) !== (certSize === 0)
      || sectionEnd > bytes.length
      || (certOffset !== 0 && (certOffset < sectionEnd || certOffset + certSize !== bytes.length))) {
    throw new Error('invalid PE certificate extent');
  }
  const bodyEnd = certOffset || bytes.length;
  const hash = crypto.createHash('sha256');
  hash.update(bytes.subarray(0, checksum));
  hash.update(bytes.subarray(checksum + 4, security));
  hash.update(bytes.subarray(security + 8, bodyEnd));
  return hash.digest('hex');
}

function verifyWindowsPackageSignature(binary, peer) {
  const systemRoot = process.env.SystemRoot || process.env.WINDIR;
  if (!systemRoot || !path.isAbsolute(systemRoot)) throw new Error('Windows system directory is unavailable');
  const powershell = path.join(systemRoot, 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe');
  const script = "$a=Get-AuthenticodeSignature -LiteralPath $env:LINGXI_VERIFY_BINARY; "
    + "$b=Get-AuthenticodeSignature -LiteralPath $env:LINGXI_VERIFY_PEER; "
    + "if($a.Status -ne 'Valid' -or $b.Status -ne 'Valid' -or "
    + "!$a.SignerCertificate -or !$b.SignerCertificate -or "
    + "$a.SignerCertificate.Thumbprint -ne $b.SignerCertificate.Thumbprint){exit 1}";
  execFileSync(powershell, ['-NoProfile', '-NonInteractive', '-Command', script], {
    env: { ...process.env, LINGXI_VERIFY_BINARY: binary, LINGXI_VERIFY_PEER: peer },
    windowsHide: true, timeout: 10000, stdio: 'pipe',
  });
}

function verifyRustServiceDirectory({
  directory, platform = process.platform, arch = process.arch,
  appVersion = null,
  allowSignedMacMutation = false, allowSignedWindowsMutation = false,
  windowsSigningPeer = null,
}) {
  const expectedPlatform = rustServicePlatformTag(platform);
  if (arch !== 'x64' && arch !== 'arm64') throw new Error(`unsupported Rust desktop architecture: ${arch}`);
  if (fs.lstatSync(directory).isSymbolicLink()) throw new Error('Rust service resource directory is a symbolic link');
  const manifestPath = path.join(directory, 'build.json');
  const manifestStat = fs.lstatSync(manifestPath);
  if (!manifestStat.isFile() || manifestStat.size < 1 || manifestStat.size > 4096) {
    throw new Error('Rust service build manifest is missing or invalid');
  }
  const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
  const filename = rustServiceExecutable(platform);
  if (manifest.schemaVersion !== 1 || manifest.platform !== expectedPlatform
      || manifest.arch !== arch || manifest.binary !== filename
      || typeof manifest.target !== 'string' || !/^[a-zA-Z0-9_-]+$/.test(manifest.target)
      || typeof manifest.toolchain !== 'string' || !/^\d+\.\d+\.\d+$/.test(manifest.toolchain)
      || typeof manifest.appVersion !== 'string' || !/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(manifest.appVersion)
      || (appVersion !== null && manifest.appVersion !== appVersion)
      || typeof manifest.sha256 !== 'string' || !/^[0-9a-f]{64}$/.test(manifest.sha256)
      || typeof manifest.contentSha256 !== 'string' || !/^[0-9a-f]{64}$/.test(manifest.contentSha256)
      || typeof manifest.sourceSha256 !== 'string' || !/^[0-9a-f]{64}$/.test(manifest.sourceSha256)) {
    throw new Error('Rust service build manifest does not match this platform and architecture');
  }
  const binary = path.join(directory, filename);
  const stat = fs.lstatSync(binary);
  if (!stat.isFile() || stat.size < 1024) throw new Error('Rust service resource is not a regular executable');
  const bytes = fs.readFileSync(binary);
  assertRustExecutableFormat(bytes, platform, arch);
  const digest = crypto.createHash('sha256').update(bytes).digest('hex');
  if (digest !== manifest.sha256) {
    if (platform === 'darwin' && allowSignedMacMutation
        && rustServiceContentDigest(bytes, platform) === manifest.contentSha256) {
      // 签名后的正文必须与构建前一致，再核整个应用封印及裸程序签名。
      const appBundle = path.resolve(directory, '..', '..', '..');
      execFileSync('codesign', ['--verify', '--strict', binary], { stdio: 'pipe' });
      execFileSync('codesign', ['--verify', '--deep', '--strict', appBundle], { stdio: 'pipe' });
    } else if (platform === 'win32' && allowSignedWindowsMutation
        && rustServiceContentDigest(bytes, platform) === manifest.contentSha256) {
      // Windows 签名只可改校验和与证书表；运行时再核同主程序签名者。
    } else {
      throw new Error('Rust service resource digest mismatch');
    }
  }
  if (platform === 'win32' && windowsSigningPeer) verifyWindowsPackageSignature(binary, windowsSigningPeer);
  if (platform !== 'win32') fs.accessSync(binary, fs.constants.X_OK);
  return { binary, manifest };
}

function rustDesktopEnabled(env = process.env) {
  const runtime = env.LINGXI_DESKTOP_SERVER_RUNTIME || 'node';
  if (runtime !== 'node' && runtime !== 'rust') {
    throw new Error(`invalid LINGXI_DESKTOP_SERVER_RUNTIME: ${runtime}`);
  }
  return runtime === 'rust';
}

function resolveRustBinary({ root, packaged, resourcesPath = process.resourcesPath, appVersion = null, env = process.env }) {
  if (packaged) {
    if (!resourcesPath || !path.isAbsolute(resourcesPath)) throw new Error('packaged Rust service resources path is missing');
    // 打包版固定使用安装包内已核对的程序；环境变量不能替换分发程序。
    return verifyRustServiceDirectory({
      directory: path.join(resourcesPath, 'rust-service'),
      appVersion,
      allowSignedMacMutation: true,
      allowSignedWindowsMutation: true,
      windowsSigningPeer: process.platform === 'win32' ? process.execPath : null,
    }).binary;
  }
  const binary = env.LINGXI_SERVICE_BIN
    || path.join(root, 'rust', 'target', 'debug', rustServiceExecutable());
  if (!binary || !path.isAbsolute(binary)) {
    throw new Error('Rust desktop runtime needs an absolute LINGXI_SERVICE_BIN');
  }
  const stat = fs.statSync(binary);
  if (!stat.isFile()) throw new Error('Rust service binary is not a regular file');
  if (process.platform !== 'win32') fs.accessSync(binary, fs.constants.X_OK);
  return binary;
}

function readPrivateJson(file, requirePrivate, windowsReaderBinary) {
  if (process.platform === 'win32') {
    // 令牌与实例记录均交给已选定的 Rust 程序按同一句柄核权限并读取。
    const before = fs.lstatSync(file);
    if (!before.isFile() || before.isSymbolicLink()) throw new Error('unsafe Rust runtime record');
    const runtime = path.dirname(file);
    const name = path.basename(file) === 'instance.json' ? 'instance'
      : path.basename(file) === 'local-token.json' ? 'local-token' : null;
    if (!name || path.basename(runtime) !== 'lingxi-service'
        || !windowsReaderBinary || !path.isAbsolute(windowsReaderBinary)) {
      throw new Error('trusted Windows Rust runtime reader is unavailable');
    }
    try {
      const bytes = execFileSync(windowsReaderBinary,
        ['--read-private-runtime-json', path.dirname(runtime), name], {
          encoding: 'utf8', timeout: 10000, maxBuffer: 65536,
          windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'],
        });
      return JSON.parse(bytes);
    } catch {
      throw new Error('Windows Rust runtime record failed private handle verification or read');
    }
  }
  const flags = fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW || 0);
  const before = fs.lstatSync(file);
  if (!before.isFile() || before.isSymbolicLink()) throw new Error('symbolic link or non-file runtime record refused');
  const fd = fs.openSync(file, flags);
  try {
    const stat = fs.fstatSync(fd);
    // Windows 没有 O_NOFOLLOW；至少证明打开的句柄与检查前后的路径身份相同。
    const after = fs.lstatSync(file);
    if (!after.isFile() || after.isSymbolicLink()
        || before.dev !== stat.dev || before.ino !== stat.ino
        || after.dev !== stat.dev || after.ino !== stat.ino) {
      throw new Error('runtime record changed while opening');
    }
    if (!stat.isFile() || stat.size <= 0 || stat.size > 65536) {
      throw new Error('runtime record is not a bounded regular file');
    }
    if (requirePrivate && process.platform !== 'win32' && (
      (stat.mode & 0o077) !== 0
      || (typeof process.getuid === 'function' && stat.uid !== process.getuid())
    )) {
      throw new Error('local token record is not owner only');
    }
    return JSON.parse(fs.readFileSync(fd, 'utf8'));
  } finally {
    fs.closeSync(fd);
  }
}

function localAddress(bindAddr, transport) {
  if (transport !== 'http' && transport !== 'https') throw new Error('invalid Rust transport');
  const url = new URL(`${transport}://${bindAddr}`);
  if (!['127.0.0.1', '[::1]'].includes(url.hostname)
      || !Number.isInteger(Number(url.port)) || Number(url.port) < 1 || Number(url.port) > 65535
      || url.username || url.password || url.pathname !== '/' || url.search || url.hash) {
    throw new Error('Rust service is not bound to a local address');
  }
  return { port: Number(url.port), transport, baseUrl: url.origin };
}

function readOwnedRustConnection({ home, pid, readyAddr, windowsReaderBinary }) {
  const runtime = path.join(home, 'lingxi-service');
  const record = readPrivateJson(path.join(runtime, 'instance.json'), false, windowsReaderBinary);
  const credential = readPrivateJson(path.join(runtime, 'local-token.json'), true, windowsReaderBinary);
  if (!record || typeof record !== 'object' || !credential || typeof credential !== 'object') {
    throw new Error('Rust runtime records are invalid');
  }
  if (record.serverKind !== 'lingxi-service' || record.pid !== pid
      || record.homePath !== fs.realpathSync(home)
      || !/^[0-9a-f]{32}$/.test(record.instanceId || '')
      || credential.kind !== 'local_token' || credential.instanceId !== record.instanceId
      || !/^[0-9a-f]{32}$/.test(credential.token || '')
      || record.bindAddr !== readyAddr) {
    throw new Error('Rust runtime instance, token, home, or READY address mismatch');
  }
  const address = localAddress(record.bindAddr, record.transport || 'http');
  return { ...address, token: credential.token, instanceId: record.instanceId };
}

function readExistingRustConnection({ home, windowsReaderBinary }) {
  const recordPath = path.join(home, 'lingxi-service', 'instance.json');
  let record;
  try {
    record = readPrivateJson(recordPath, false, windowsReaderBinary);
  } catch (err) {
    if (err?.code === 'ENOENT') return null;
    throw err;
  }
  if (!Number.isInteger(record?.pid) || record.pid <= 0 || typeof record.bindAddr !== 'string') {
    throw new Error('existing Rust instance record has no valid PID or address');
  }
  try {
    process.kill(record.pid, 0);
  } catch (err) {
    if (err?.code === 'ESRCH') return null;
    throw err;
  }
  return { ...readOwnedRustConnection({ home, pid: record.pid, readyAddr: record.bindAddr, windowsReaderBinary }), pid: record.pid };
}

module.exports = {
  rustDesktopEnabled, rustServicePlatformTag, rustServiceExecutable,
  rustServiceContentDigest, verifyRustServiceDirectory, resolveRustBinary,
  readOwnedRustConnection, readExistingRustConnection,
};
