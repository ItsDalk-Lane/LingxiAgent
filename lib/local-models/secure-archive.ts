import fs from "node:fs";
import fsp from "node:fs/promises";
import path from "node:path";
import { Transform } from "node:stream";
import { pipeline } from "node:stream/promises";
import yauzl from "yauzl";
import { LocalModelError, throwIfAborted } from "./errors.ts";

const IFMT = 0o170000;
const IFDIR = 0o040000;
const IFLNK = 0o120000;

export interface SecureArchiveLimits {
  maxEntries: number;
  maxFileBytes: number;
  maxTotalBytes: number;
  maxCompressionRatio: number;
}

export interface SecureExtractOptions {
  signal: AbortSignal;
  expectedEntries: readonly string[];
  limits?: Partial<SecureArchiveLimits>;
  onEntry?: (entry: { path: string; bytes: number }) => void;
}

export interface SecureExtractResult {
  files: ReadonlyArray<{ path: string; bytes: number }>;
  totalBytes: number;
}

const DEFAULT_LIMITS: SecureArchiveLimits = Object.freeze({
  maxEntries: 10_000,
  maxFileBytes: 8 * 1024 * 1024 * 1024,
  maxTotalBytes: 16 * 1024 * 1024 * 1024,
  maxCompressionRatio: 200,
});

/**
 * 流式解压模型 ZIP 到调用方专属的空 staging 目录。
 * 失败时会删除该 staging 目录；调用方不得传入含用户文件的既有目录。
 */
export async function extractLocalModelZip(
  zipPath: string,
  destination: string,
  options: SecureExtractOptions,
): Promise<SecureExtractResult> {
  const root = path.resolve(destination);
  const limits = { ...DEFAULT_LIMITS, ...(options.limits || {}) };
  validateLimits(limits);
  throwIfAborted(options.signal);
  await prepareOwnedEmptyDirectory(root);

  const expected = new Map<string, string>();
  for (const entry of options.expectedEntries) {
    const safe = normalizeArchivePath(entry);
    const folded = safe.toLocaleLowerCase("en-US");
    if (expected.has(folded)) unsafe("expected entry list contains a case-insensitive duplicate", { entry: safe });
    expected.set(folded, safe);
  }
  if (expected.size === 0) unsafe("expected entry list must not be empty");

  const zipfile = await openZip(zipPath);
  const seen = new Set<string>();
  const files: Array<{ path: string; bytes: number }> = [];
  let totalBytes = 0;
  let entryCount = 0;
  try {
    await new Promise<void>((resolve, reject) => {
      let settled = false;
      const finish = (error?: unknown) => {
        if (settled) return;
        settled = true;
        options.signal.removeEventListener("abort", onAbort);
        if (error) reject(error);
        else resolve();
      };
      const onAbort = () => {
        zipfile.close();
        finish(new LocalModelError("LOCAL_MODEL_ABORTED", "model archive extraction was cancelled"));
      };
      options.signal.addEventListener("abort", onAbort, { once: true });
      zipfile.on("error", finish);
      zipfile.on("end", () => finish());
      zipfile.on("entry", (entry) => {
        extractEntry(entry).then(
          () => zipfile.readEntry(),
          (error) => {
            zipfile.close();
            finish(error);
          },
        );
      });
      zipfile.readEntry();
    });

    const missing = [...expected.entries()]
      .filter(([folded]) => !seen.has(folded))
      .map(([, original]) => original);
    if (missing.length > 0) unsafe("archive is missing manifest-declared entries", { missing });
    return Object.freeze({
      files: Object.freeze(files.map((entry) => Object.freeze({ ...entry }))),
      totalBytes,
    });
  } catch (error) {
    await fsp.rm(root, { recursive: true, force: true }).catch(() => {});
    throw error;
  } finally {
    zipfile.close();
  }

  async function extractEntry(entry: yauzl.Entry): Promise<void> {
    throwIfAborted(options.signal);
    entryCount += 1;
    if (entryCount > limits.maxEntries) unsafe("archive contains too many entries", { maxEntries: limits.maxEntries });
    const relative = normalizeArchivePath(entry.fileName);
    if (relative.startsWith("__MACOSX/")) return;
    const mode = (entry.externalFileAttributes >>> 16) & 0xffff;
    if ((mode & IFMT) === IFLNK) unsafe("archive contains a symbolic link", { entry: relative });
    const directory = (mode & IFMT) === IFDIR
      || relative.endsWith("/")
      || ((entry.versionMadeBy >>> 8) === 0 && entry.externalFileAttributes === 16);
    const target = resolveInside(root, relative);
    if (directory) {
      await ensureSafeDirectory(root, target);
      return;
    }

    const folded = relative.toLocaleLowerCase("en-US");
    if (seen.has(folded)) unsafe("archive contains a duplicate destination", { entry: relative });
    if (!expected.has(folded) || expected.get(folded) !== relative) {
      unsafe("archive contains an entry not declared by the manifest", { entry: relative });
    }
    if (entry.uncompressedSize > limits.maxFileBytes) {
      unsafe("archive entry exceeds the per-file limit", { entry: relative, bytes: entry.uncompressedSize });
    }
    if (entry.compressedSize > 0 && entry.uncompressedSize / entry.compressedSize > limits.maxCompressionRatio) {
      unsafe("archive entry exceeds the compression-ratio limit", {
        entry: relative,
        ratio: entry.uncompressedSize / entry.compressedSize,
      });
    }
    if (totalBytes + entry.uncompressedSize > limits.maxTotalBytes) {
      unsafe("archive exceeds the total uncompressed limit", { maxTotalBytes: limits.maxTotalBytes });
    }

    await ensureSafeDirectory(root, path.dirname(target));
    const stream = await openReadStream(zipfile, entry);
    let written = 0;
    let crc = 0xffffffff;
    const meter = new Transform({
      transform(chunk: Buffer, _encoding, callback) {
        try {
          throwIfAborted(options.signal);
          written += chunk.length;
          if (written > limits.maxFileBytes || totalBytes + written > limits.maxTotalBytes) {
            unsafe("archive output exceeded the declared safety limit", { entry: relative });
          }
          crc = updateCrc32(crc, chunk);
          callback(null, chunk);
        } catch (error) {
          callback(error as Error);
        }
      },
    });
    const writer = fs.createWriteStream(target, {
      flags: "wx",
      mode: ((mode || 0o644) & 0o777) & ~0o6000,
    });
    await pipeline(stream, meter, writer, { signal: options.signal });
    const finalCrc = (crc ^ 0xffffffff) >>> 0;
    if (written !== entry.uncompressedSize) {
      unsafe("archive entry size did not match its central directory", {
        entry: relative,
        expected: entry.uncompressedSize,
        actual: written,
      });
    }
    if (finalCrc !== (entry.crc32 >>> 0)) {
      unsafe("archive entry CRC32 check failed", { entry: relative });
    }
    totalBytes += written;
    seen.add(folded);
    files.push({ path: relative, bytes: written });
    options.onEntry?.({ path: relative, bytes: written });
  }
}

function openZip(zipPath: string): Promise<yauzl.ZipFile> {
  return new Promise((resolve, reject) => {
    yauzl.open(zipPath, { lazyEntries: true, validateEntrySizes: true }, (error, zipfile) => {
      if (error || !zipfile) reject(error || new Error("could not open zip"));
      else resolve(zipfile);
    });
  });
}

function openReadStream(zipfile: yauzl.ZipFile, entry: yauzl.Entry): Promise<NodeJS.ReadableStream> {
  return new Promise((resolve, reject) => {
    zipfile.openReadStream(entry, (error, stream) => {
      if (error || !stream) reject(error || new Error("could not read zip entry"));
      else resolve(stream);
    });
  });
}

async function prepareOwnedEmptyDirectory(root: string): Promise<void> {
  const stat = await fsp.lstat(root).catch((error: NodeJS.ErrnoException) => {
    if (error.code === "ENOENT") return null;
    throw error;
  });
  if (stat?.isSymbolicLink()) unsafe("extraction destination must not be a symbolic link");
  if (stat && !stat.isDirectory()) unsafe("extraction destination must be a directory");
  if (stat && (await fsp.readdir(root)).length > 0) unsafe("extraction destination must be empty");
  await fsp.mkdir(root, { recursive: true, mode: 0o700 });
}

async function ensureSafeDirectory(root: string, directory: string): Promise<void> {
  const resolved = resolveInside(root, path.relative(root, directory) || ".");
  await fsp.mkdir(resolved, { recursive: true, mode: 0o755 });
  let current = root;
  const relative = path.relative(root, resolved);
  for (const segment of relative.split(path.sep).filter(Boolean)) {
    current = path.join(current, segment);
    const stat = await fsp.lstat(current);
    if (stat.isSymbolicLink() || !stat.isDirectory()) {
      unsafe("archive destination parent is not a real directory", { path: path.relative(root, current) });
    }
  }
}

function resolveInside(root: string, relative: string): string {
  const target = path.resolve(root, relative);
  if (target !== root && !target.startsWith(root + path.sep)) {
    unsafe("archive entry escapes the extraction root", { entry: relative });
  }
  return target;
}

function normalizeArchivePath(value: string): string {
  if (typeof value !== "string" || !value || value.includes("\0")) unsafe("archive entry has an invalid name");
  const normalized = value.replaceAll("\\", "/");
  if (normalized.startsWith("/") || /^[A-Za-z]:\//.test(normalized)) {
    unsafe("archive entry uses an absolute path", { entry: value });
  }
  const parts = normalized.split("/");
  if (parts.some((part) => part === ".." || part === "" && normalized !== parts.join("/"))) {
    unsafe("archive entry contains an unsafe path segment", { entry: value });
  }
  return parts.filter(Boolean).join("/") + (normalized.endsWith("/") ? "/" : "");
}

function validateLimits(limits: SecureArchiveLimits): void {
  for (const [name, value] of Object.entries(limits)) {
    if (!Number.isFinite(value) || value <= 0) unsafe(`invalid archive safety limit ${name}`);
  }
}

function unsafe(message: string, details: Record<string, unknown> = {}): never {
  throw new LocalModelError("LOCAL_MODEL_ARCHIVE_UNSAFE", message, details);
}

const CRC32_TABLE = new Uint32Array(256).map((_, index) => {
  let value = index;
  for (let bit = 0; bit < 8; bit += 1) value = (value & 1) ? (0xedb88320 ^ (value >>> 1)) : (value >>> 1);
  return value >>> 0;
});

function updateCrc32(initial: number, chunk: Uint8Array): number {
  let crc = initial >>> 0;
  for (const byte of chunk) crc = CRC32_TABLE[(crc ^ byte) & 0xff] ^ (crc >>> 8);
  return crc >>> 0;
}

