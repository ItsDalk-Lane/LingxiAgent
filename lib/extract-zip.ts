/**
 * extract-zip.js — 跨平台 zip 解压
 *
 * 直接使用应用自带的 JS 解压能力，避免桌面/服务端把核心安装链路外包给
 * 系统环境里的 unzip / PowerShell。
 *
 * 实现说明：底层走 yauzl，逐 entry 顺序读取（flowing 模式缓冲单个 entry 后
 * 落盘）。不要使用 extract-zip@2.0.1 包：它的 extract-zip → yauzl → fd-slicer
 * 管道在 entry 大于 highWaterMark（触发背压）时会永久挂起——fd-slicer 用
 * pend(max=1) 串行化 fd 读取，背压暂停后读流不再恢复，await 永远不 resolve。
 *
 * 安全约束：拒绝任何带 symlink entry 的 zip。创建 symlink 时不校验 link
 * target 的边界，且后续同名 file entry 会沿 symlink 解引用写穿到任意可写
 * 路径（zip-slip via symlink）。本项目的所有合法解压用例（角色卡、插件、
 * 技能、desk skill）都不需要 symlink entry。entry 文件名的绝对路径 / ".."
 * 穿越由 yauzl 的 validateFileName 直接拒绝，这里再做一次落盘路径的包含性
 * 校验作为纵深防御。
 */

import fsp from "fs/promises";
import path from "path";
import yauzl from "yauzl";

const IFMT = 0o170000;
const IFDIR = 0o040000;
const IFLNK = 0o120000;

function openZip(zipPath, options): Promise<yauzl.ZipFile> {
  return new Promise((resolve, reject) => {
    yauzl.open(zipPath, options, (err, zipfile) => {
      if (err) reject(err);
      else resolve(zipfile);
    });
  });
}

export function isSymlinkEntry(entry) {
  if (!entry || typeof entry.externalFileAttributes !== "number") return false;
  const mode = (entry.externalFileAttributes >> 16) & 0xFFFF;
  return (mode & IFMT) === IFLNK;
}

function rejectSymlinkEntries(entry) {
  if (isSymlinkEntry(entry)) {
    const name = entry?.fileName || "<unnamed>";
    throw new Error(`extract-zip: symlink entry is not allowed (entry: ${name})`);
  }
}

function readEntryData(zipfile, entry) {
  return new Promise<Buffer>((resolve, reject) => {
    zipfile.openReadStream(entry, (err, readStream) => {
      if (err) return reject(err);
      const chunks = [];
      readStream.on("data", (chunk) => chunks.push(chunk));
      readStream.on("end", () => resolve(Buffer.concat(chunks)));
      readStream.on("error", reject);
    });
  });
}

export async function extractZip(zipPath, destDir) {
  const rootDir = path.resolve(destDir);
  await fsp.mkdir(rootDir, { recursive: true });
  const zipfile = await openZip(zipPath, { lazyEntries: true });
  try {
    await new Promise((resolve, reject) => {
      zipfile.on("error", reject);
      zipfile.on("end", resolve);
      zipfile.on("entry", (entry) => {
        extractEntry(entry).then(
          () => zipfile.readEntry(),
          reject,
        );
      });
      zipfile.readEntry();
    });
  } finally {
    zipfile.close();
  }

  async function extractEntry(entry) {
    rejectSymlinkEntries(entry);
    const fileName = entry.fileName;
    if (fileName.startsWith("__MACOSX/")) return;

    const mode = (entry.externalFileAttributes >> 16) & 0xFFFF;
    let isDir = (mode & IFMT) === IFDIR;
    if (!isDir && fileName.endsWith("/")) isDir = true;
    // Windows 归档工具的目录标记（同 extract-zip 的处理）
    const madeBy = entry.versionMadeBy >> 8;
    if (!isDir && madeBy === 0 && entry.externalFileAttributes === 16) isDir = true;

    const dest = path.resolve(rootDir, fileName);
    if (dest !== rootDir && !dest.startsWith(rootDir + path.sep)) {
      throw new Error(`extract-zip: entry escapes the target directory (entry: ${fileName})`);
    }

    if (isDir) {
      await fsp.mkdir(dest, { recursive: true, mode: (mode || 0o755) & 0o777 });
      return;
    }
    await fsp.mkdir(path.dirname(dest), { recursive: true });
    const data = await readEntryData(zipfile, entry);
    await fsp.writeFile(dest, data);
    await fsp.chmod(dest, (mode || 0o644) & 0o777);
  }
}
