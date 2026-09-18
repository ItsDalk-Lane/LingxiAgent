/**
 * security_scan 三件套测试：规则表命中/脱敏边界、基线扫描跳过策略、
 * 工具的两种模式/范围硬校验/审计计数（不进正文）/外部扫描器缺席如实。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

vi.mock("../../env-deps/detect.ts", () => ({
  detectEnvDeps: vi.fn(async () => ({ deps: [] })),
}));

import { scanTextForSecrets, SECRET_RULES } from "../../security/secret-rules.ts";
import { scanFilesForSecrets, sortFindings, countBySeverity } from "../../security/baseline-scanner.ts";
import { createSecurityScanTool } from "../security-scan-tool.ts";

const roots: string[] = [];
function freshRoot() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-secscan-"));
  roots.push(root);
  return root;
}
afterEach(() => { while (roots.length) fs.rmSync(roots.pop()!, { recursive: true, force: true }); });

describe("secret-rules", () => {
  it("规则表覆盖 critical/warning/info 三级", () => {
    const severities = new Set(SECRET_RULES.map((r) => r.severity));
    expect(severities.has("critical")).toBe(true);
    expect(severities.has("warning")).toBe(true);
    expect(severities.has("info")).toBe(true);
  });

  it("命中各类密钥并给行号；同行同规则去重", () => {
    const text = [
      "const clean = 1;",
      'const key = "sk-abcdefghijklmnopqrstuvwxyz012345";',
      'const key2 = "sk-abcdefghijklmnopqrstuvwxyz012345"; // 同类第二处（不同行）',
      "-----BEGIN RSA PRIVATE KEY-----",
      "const url = 'https://api.test?token=abcdef123'",
      "身份证示例不在测试里放真实号段",
    ].join("\n");
    const hits = scanTextForSecrets(text);
    const ids = hits.map((h) => h.ruleId);
    expect(ids).toContain("secret_api_key_literal");
    expect(ids.some((id) => id.startsWith("pii_private_key"))).toBe(true);
    expect(ids).toContain("secret_url_query");
    const apiHit = hits.find((h) => h.ruleId === "secret_api_key_literal");
    expect(apiHit?.line).toBe(2);
    // 命中结构永不携带正文
    expect(JSON.stringify(hits)).not.toContain("sk-alphab");
  });
});

describe("baseline-scanner", () => {
  it("二进制（含 NUL）跳过并计数；空文本不算二进制", () => {
    const result = scanFilesForSecrets([
      { path: "bin.dat", content: "abc\0def" },
      { path: "ok.ts", content: "const a = 1;\n" },
      { path: "empty.txt", content: "" },
    ]);
    expect(result.skipped.binary).toBe(1);
    expect(result.filesScanned).toBe(2);
    expect(result.findings).toEqual([]);
  });

  it("排序按严重度优先，计数分级", () => {
    const findings = [
      { file: "b.ts", ruleId: "r1", severity: "warning" as const, line: 3 },
      { file: "a.ts", ruleId: "r2", severity: "critical" as const, line: 1 },
      { file: "c.ts", ruleId: "r3", severity: "info" as const, line: 9 },
    ];
    const sorted = sortFindings(findings);
    expect(sorted[0].severity).toBe("critical");
    expect(countBySeverity(findings)).toEqual({ critical: 1, warning: 1, info: 1 });
  });
});

describe("security_scan 工具", () => {
  beforeEach(() => { vi.clearAllMocks(); });

  function makeTool(root: string, opts: any = {}) {
    const audits: any[] = [];
    const tool = createSecurityScanTool({
      cwd: root,
      getAuthorizedFolders: () => opts.authorizedFolders || [],
      getLingxiHome: () => null,
      appendAudit: (e) => audits.push(e),
    });
    return { tool, audits };
  }

  it("path 模式：扫目录、命中密钥、审计只进计数", async () => {
    const root = freshRoot();
    fs.mkdirSync(path.join(root, "src"), { recursive: true });
    fs.writeFileSync(path.join(root, "src", "leak.ts"), 'const api = "sk-abcdefghijklmnopqrstuvwxyz1234";\n');
    fs.writeFileSync(path.join(root, "src", "clean.ts"), "const ok = 1;\n");
    fs.mkdirSync(path.join(root, "node_modules"), { recursive: true });
    fs.writeFileSync(path.join(root, "node_modules", "dep.js"), 'const x = "sk-abcdefghijklmnopqrstuvwxyz1234";\n');

    const { tool, audits } = makeTool(root);
    const r: any = await tool.execute("c1", { mode: "path", path: "." });
    expect(r.details.counts.critical).toBe(1);
    expect(r.content[0].text).toContain("src/leak.ts:1");
    expect(r.content[0].text).not.toContain("sk-alphab"); // 结果不携带命中正文
    expect(r.details.filesScanned).toBe(2); // node_modules 被跳过
    expect(audits).toHaveLength(1);
    expect(audits[0].action).toBe("security_scan");
    expect(JSON.stringify(audits[0])).not.toContain("sk-alphab");
  });

  it("范围硬校验：path 指到授权文件夹之外直接拒绝", async () => {
    const root = freshRoot();
    const outside = fs.mkdtempSync(path.join(os.tmpdir(), "hana-outside-"));
    roots.push(outside);
    const { tool } = makeTool(root);
    const r: any = await tool.execute("c1", { mode: "path", path: outside });
    expect(r.isError).toBe(true);
    expect(r.details.errorCode).toBe("SECURITY_SCAN_OUT_OF_SCOPE");
  });

  it("授权文件夹内的目标放行", async () => {
    const root = freshRoot();
    const extra = fs.mkdtempSync(path.join(os.tmpdir(), "hana-extra-"));
    roots.push(extra);
    fs.writeFileSync(path.join(extra, "a.txt"), "clean\n");
    const { tool } = makeTool(root, { authorizedFolders: [extra] });
    const r: any = await tool.execute("c1", { mode: "path", path: extra });
    expect(r.details.errorCode).toBeUndefined();
    expect(r.details.filesScanned).toBe(1);
  });

  it("changes 模式：非 git 目录如实说明并建议 path 模式", async () => {
    const root = freshRoot();
    const { tool } = makeTool(root);
    const r: any = await tool.execute("c1", { mode: "changes" });
    expect(r.content[0].text).toContain("not a git repository");
    expect(r.content[0].text).toContain("mode=path");
  });

  it("外部扫描器未装：注明 baseline-only，不报错", async () => {
    const root = freshRoot();
    fs.writeFileSync(path.join(root, "a.ts"), "clean\n");
    const { tool } = makeTool(root);
    const r: any = await tool.execute("c1", { mode: "path", path: "." });
    expect(r.details.engines).toEqual(["baseline"]);
    expect(r.content[0].text).toContain("baseline only");
  });

  it("权限契约：read（计划模式可用）", () => {
    const { tool } = makeTool(freshRoot());
    expect(tool.sessionPermission.resolveInvocation({})).toEqual({
      action: "scan", kind: "read", capability: "security_scan.scan",
    });
  });
});
