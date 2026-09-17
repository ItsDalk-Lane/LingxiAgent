/**
 * external-scanners.ts — semgrep / gitleaks 环境增强（阶段二·9）。
 *
 * 装了就用（execFile 直调，不走模型的命令执行器）；没装如实缺席（基线
 * 扫描照常）。可用性经 lib/env-deps 探测缓存查询。命中归一成与基线同一
 * finding 结构；不保留扫描器原文输出里的代码片段（只留 file/line/ruleId
 * /severity），审计与结果展示同源同脱敏。
 */
import { execFile } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { promisify } from "node:util";
import { detectEnvDeps } from "../env-deps/detect.ts";
import type { SecretFinding, SecretSeverity } from "./secret-rules.ts";

const execFileAsync = promisify(execFile);
const EXTERNAL_TIMEOUT_MS = 120_000;

export interface ExternalScannerAvailability {
  semgrep: boolean;
  gitleaks: boolean;
}

export async function queryExternalScannerAvailability(): Promise<ExternalScannerAvailability> {
  try {
    const report = await detectEnvDeps({});
    const installed = (id: string) => report.deps?.some?.((d: any) => d?.id === id && d?.status === "installed") === true;
    return { semgrep: installed("semgrep"), gitleaks: installed("gitleaks") };
  } catch {
    return { semgrep: false, gitleaks: false };
  }
}

function severityFor(ruleId: string, source: string): SecretSeverity {
  const rid = ruleId.toLowerCase();
  if (rid.includes("secret") || rid.includes("key") || rid.includes("token") || rid.includes("credential") || rid.includes("password")) {
    return source === "gitleaks" ? "critical" : "warning";
  }
  return "warning";
}

/** gitleaks（detect 源码目录）→ findings。报告走临时文件，避免 stdout 截断。 */
export async function runGitleaks(cwd: string): Promise<SecretFinding[]> {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "hana-gitleaks-"));
  try {
    const reportPath = path.join(tmp, "report.json");
    await execFileAsync("gitleaks", [
      "detect", "--source", cwd, "--report-format", "json", "--report-path", reportPath,
      "--redact", "--no-banner", "--exit-code", "0",
    ], { timeout: EXTERNAL_TIMEOUT_MS, maxBuffer: 16 * 1024 * 1024 });
    const raw = JSON.parse(fs.readFileSync(reportPath, "utf8"));
    if (!Array.isArray(raw)) return [];
    const findings: SecretFinding[] = [];
    for (const item of raw) {
      const file = typeof item?.File === "string" ? item.File : "";
      const line = Number(item?.StartLine);
      const ruleId = typeof item?.RuleID === "string" && item.RuleID ? item.RuleID : "gitleaks_finding";
      if (!file || !Number.isFinite(line)) continue;
      findings.push({
        file: path.relative(cwd, file) || file,
        ruleId: `gitleaks:${ruleId}`,
        severity: severityFor(ruleId, "gitleaks"),
        line,
      });
    }
    return findings;
  } catch {
    // gitleaks 跑挂（规则/版本差异）→ 如实缺席，不阻塞基线结果
    return [];
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

/** semgrep（指定文件清单或整目录）→ findings。 */
export async function runSemgrep(cwd: string, targets: string[] = []): Promise<SecretFinding[]> {
  const args = ["scan", "--json", "--quiet", "--timeout", "60"];
  if (targets.length) args.push(...targets);
  else args.push(cwd);
  try {
    const { stdout } = await execFileAsync("semgrep", args, {
      timeout: EXTERNAL_TIMEOUT_MS,
      maxBuffer: 32 * 1024 * 1024,
      cwd,
    });
    const raw = JSON.parse(stdout);
    const results = Array.isArray(raw?.results) ? raw.results : [];
    const findings: SecretFinding[] = [];
    for (const item of results) {
      const file = typeof item?.path === "string" ? item.path : "";
      const line = Number(item?.start?.line);
      const ruleId = typeof item?.check_id === "string" && item.check_id ? item.check_id : "semgrep_finding";
      if (!file || !Number.isFinite(line)) continue;
      findings.push({
        file,
        ruleId: `semgrep:${ruleId}`,
        severity: severityFor(ruleId, "semgrep"),
        line,
      });
    }
    return findings;
  } catch {
    return [];
  }
}
