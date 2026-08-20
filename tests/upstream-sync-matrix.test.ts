/**
 * 133-path upstream sync matrix — Gate A 的机器校验。
 *
 * 权威输入是 git 重新计算的 ΔU（.sync-audit/delta-U-final.txt）和机器真相源
 * （.sync-audit/upstream-sync-matrix.json）；UPSTREAM_SYNC_MATRIX.md 只是投影，
 * 这里用投影哈希防止"改了 JSON 忘了重新生成 MD"。
 *
 * 不变量（对应任务书 Gate A）：
 *   ΔU = 133 paths；matrix rows = 133 且 unique；missing/extra/duplicate = 0；
 *   disposition 只取 ADOPTED/ADAPTED/REGENERATED/INTENTIONAL_DIVERGENCE；
 *   四类之和 = 133；每行 test_evidence 具体（不允许 full suite 式占位）。
 */
import crypto from "crypto";
import fs from "fs";
import path from "path";
import { describe, expect, it } from "vitest";

const ROOT = path.resolve(__dirname, "..");
const DELTA_FILE = path.join(ROOT, ".sync-audit", "delta-U-final.txt");
const JSON_FILE = path.join(ROOT, ".sync-audit", "upstream-sync-matrix.json");
const MD_FILE = path.join(ROOT, "UPSTREAM_SYNC_MATRIX.md");

const ALLOWED_DISPOSITIONS = ["ADOPTED", "ADAPTED", "REGENERATED", "INTENTIONAL_DIVERGENCE"];

function deltaPaths() {
  return fs.readFileSync(DELTA_FILE, "utf-8")
    .split("\n")
    .filter(Boolean)
    .map((line) => {
      const parts = line.split("\t");
      return parts[0].startsWith("R") ? parts[2] : parts[1];
    });
}

function loadMatrix() {
  return JSON.parse(fs.readFileSync(JSON_FILE, "utf-8"));
}

describe("upstream sync matrix audit (Gate A)", () => {
  it("ΔU has exactly 133 changed paths", () => {
    const paths = deltaPaths();
    expect(paths).toHaveLength(133);
    expect(new Set(paths).size).toBe(133);
  });

  it("matrix covers every ΔU path exactly once, with no extras", () => {
    const paths = deltaPaths();
    const matrix = loadMatrix();
    const rows = matrix.records.map((record: any) => record.upstream_path);

    expect(rows).toHaveLength(133);
    expect(new Set(rows).size).toBe(133);

    const rowSet = new Set(rows);
    const missing = paths.filter((p) => !rowSet.has(p));
    const extra = rows.filter((p: string) => !new Set(paths).has(p));
    expect(missing).toEqual([]);
    expect(extra).toEqual([]);
  });

  it("uses only the four legal dispositions and the counts sum to 133", () => {
    const matrix = loadMatrix();
    for (const record of matrix.records) {
      expect(ALLOWED_DISPOSITIONS).toContain(record.disposition);
    }
    const counts = Object.fromEntries(ALLOWED_DISPOSITIONS.map((d) => [d, 0]));
    for (const record of matrix.records) counts[record.disposition] += 1;
    const sum = Object.values<number>(counts).reduce((a, b) => a + b, 0);
    expect(sum).toBe(133);
    // summary 必须由记录计算得出，不允许与记录脱节
    expect(matrix.summary).toMatchObject(counts);
    expect(matrix.summary.total).toBe(133);
  });

  it("every row carries concrete test evidence and a closing status", () => {
    const matrix = loadMatrix();
    for (const record of matrix.records) {
      expect(record.status, record.upstream_path).toBe("✅");
      expect(typeof record.test_evidence, record.upstream_path).toBe("string");
      expect(record.test_evidence.trim().length, record.upstream_path).toBeGreaterThan(0);
      expect(record.test_evidence.trim(), record.upstream_path)
        .not.toMatch(/^(full suite|全量测试|all green)\.?$/i);
      expect(["A", "B", "C", "D"]).toContain(record.conflict_class);
    }
  });

  it("renames are recorded per real path with their legacy source", () => {
    const matrix = loadMatrix();
    const renames = matrix.records.filter((record: any) => record.upstream_change.startsWith("R"));
    expect(renames).toHaveLength(13);
    for (const record of renames) {
      expect(record.renamed_from, record.upstream_path).toBeTruthy();
    }
  });

  it("UPSTREAM_SYNC_MATRIX.md is a fresh projection of the machine truth source", () => {
    const matrix = loadMatrix();
    const projectionSha = crypto.createHash("sha256")
      .update(JSON.stringify({ summary: matrix.summary, records: matrix.records }))
      .digest("hex");
    const md = fs.readFileSync(MD_FILE, "utf-8");
    expect(md).toContain(`Source-JSON-SHA256: ${projectionSha}`);
    expect(md).toContain(`Total upstream paths: ${matrix.summary.total}`);
  });

  it("VERIFIED_SOURCE_SHA is a 40-hex audit coordinate, consistent across sources", () => {
    // 审计坐标：被最终验证（typecheck/lint/测试/构建/打包）所针对的源码树。
    // 它与当前 branch HEAD 无直接相等约束（允许其后存在纯审计 seal 提交），
    // 只需一致且合法；HEAD 与 VERIFIED_SOURCE_SHA 之间的 diff 由 post-verification
    // audit-seal 测试另行门禁（只允许审计文件变化）。
    const shaFile = path.join(ROOT, ".sync-audit", "verified-source-sha.txt");
    expect(fs.existsSync(shaFile)).toBe(true);
    const verifiedSourceSha = fs.readFileSync(shaFile, "utf-8").trim();
    expect(verifiedSourceSha).toMatch(/^[0-9a-f]{40}$/);

    const matrix = loadMatrix();
    expect(matrix.coordinates.VERIFIED_SOURCE_SHA, "JSON coordinates").toBe(verifiedSourceSha);
    expect(matrix.coordinates.FINAL_SHA, "legacy FINAL_SHA must be gone").toBeUndefined();

    const md = fs.readFileSync(MD_FILE, "utf-8");
    expect(md).toContain(`VERIFIED_SOURCE_SHA = ${verifiedSourceSha}`);
    expect(md).not.toContain("FINAL_SHA");

    // 审计文档坐标亦须一致。
    for (const doc of ["UPSTREAM_SYNC_AUDIT.md", "PROGRESS.md"]) {
      expect(fs.readFileSync(path.join(ROOT, doc), "utf-8"))
        .toContain(verifiedSourceSha);
    }
  });
});
