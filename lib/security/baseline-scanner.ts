/**
 * baseline-scanner.ts — security_scan 的纯 JS 基线扫描（永远可用）。
 *
 * 输入 = 文件清单（路径+内容）；范围控制由调用方（工具层）负责——本模块
 * 只扫给定文件，绝不自己walk授权边界之外。二进制（含 NUL）与超大文本
 * 跳过并如实计数。命中结构只含 file/ruleId/severity/line（不带正文，
 * 照 injection-scan 的边界手法）。
 */
import { scanTextForSecrets, type SecretFinding, type SecretSeverity } from "./secret-rules.ts";

/** 单文件扫描上限：更大的文本跳过（防把超大 minified bundle 读爆）。 */
export const MAX_SCAN_BYTES = 2 * 1024 * 1024;

export interface ScannedFileInput {
  /** 仓库相对路径（结果展示用原样返回）。 */
  path: string;
  content: string;
}

export interface BaselineScanResult {
  findings: SecretFinding[];
  filesScanned: number;
  filesSkipped: number;
  /** 跳过原因计数（binary / oversized）。 */
  skipped: { binary: number; oversized: number };
}

export function scanFilesForSecrets(files: ScannedFileInput[]): BaselineScanResult {
  const findings: SecretFinding[] = [];
  const skipped = { binary: 0, oversized: 0 };
  let filesScanned = 0;
  for (const file of files) {
    if (!file || typeof file.content !== "string") {
      skipped.binary += 1;
      continue;
    }
    if (Buffer.byteLength(file.content, "utf8") > MAX_SCAN_BYTES) {
      skipped.oversized += 1;
      continue;
    }
    if (file.content.includes("\0")) {
      skipped.binary += 1;
      continue;
    }
    filesScanned += 1;
    for (const hit of scanTextForSecrets(file.content)) {
      findings.push({ file: file.path, ...hit });
    }
  }
  return { findings, filesScanned, filesSkipped: skipped.binary + skipped.oversized, skipped };
}

const SEVERITY_ORDER: Record<SecretSeverity, number> = { critical: 0, warning: 1, info: 2 };

export function sortFindings(findings: SecretFinding[]): SecretFinding[] {
  return [...findings].sort((a, b) => (
    SEVERITY_ORDER[a.severity] - SEVERITY_ORDER[b.severity]
    || a.file.localeCompare(b.file)
    || a.line - b.line
  ));
}

export function countBySeverity(findings: SecretFinding[]): Record<SecretSeverity, number> {
  const counts = { critical: 0, warning: 0, info: 0 };
  for (const f of findings) counts[f.severity] += 1;
  return counts;
}
