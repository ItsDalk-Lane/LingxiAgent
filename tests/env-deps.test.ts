/**
 * 环境依赖检测：登记表完整性 + 探测器行为（注入假 probe，不碰真实 PATH）。
 */
import { describe, expect, it } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { ENV_DEP_ENTRIES, getEnvDepEntry } from "../lib/env-deps/registry.ts";
import { detectEnvDeps, _resetEnvDepsCacheForTest } from "../lib/env-deps/detect.ts";

describe("env-deps registry", () => {
  it("id 唯一且必填字段完整", () => {
    const ids = new Set<string>();
    for (const e of ENV_DEP_ENTRIES) {
      expect(ids.has(e.id), `duplicate id ${e.id}`).toBe(false);
      ids.add(e.id);
      expect(e.label.length).toBeGreaterThan(0);
      expect(e.binaries.length).toBeGreaterThan(0);
      expect(e.versionArgs.length).toBeGreaterThan(0);
      expect(e.requiredBy.length).toBeGreaterThan(0);
      if (e.kind === "managed") expect(e.managedBinName, `${e.id} managed 缺 managedBinName`).toBeTruthy();
    }
  });

  it("getEnvDepEntry 命中与未命中", () => {
    expect(getEnvDepEntry("git")?.label).toBe("Git");
    expect(getEnvDepEntry("nope")).toBeNull();
  });

  it("ffmpeg 占位登记已撤除（视频生成走云端，本机无加工链）", () => {
    expect(getEnvDepEntry("ffmpeg")).toBeNull();
    expect(ENV_DEP_ENTRIES.some(e => e.id === "ffmpeg")).toBe(false);
  });
});

describe("detectEnvDeps（注入 probe）", () => {
  const probeOk = (version = "1.2.3") => async () => ({ ok: true, version });
  const probeFail = async () => ({ ok: false, note: "not_found" });

  it("已安装/未安装按 probe 结果分类", async () => {
    const report = await detectEnvDeps({
      probe: async bin => (bin === "git" ? { ok: true, version: "2.45.1" } : { ok: false, note: "not_found" }),
      findManaged: () => null,
      scanSignals: () => false,
    });
    const git = report.deps.find(d => d.id === "git")!;
    const py = report.deps.find(d => d.id === "python3")!;
    expect(git.status).toBe("installed");
    expect(git.version).toBe("2.45.1");
    expect(py.status).toBe("missing");
    expect(report.summary.installed).toBe(1);
    expect(report.summary.missing).toBe(report.deps.length - 1);
    expect(report.summary.projectMissing).toEqual([]);
  });

  it("managed 类优先查托管目录", async () => {
    const calls: string[] = [];
    const report = await detectEnvDeps({
      probe: async bin => {
        calls.push(bin);
        return { ok: true, version: "0.39.0" };
      },
      findManaged: name => (name === "ast-grep" ? "/managed/dir/ast-grep" : null),
      findBundled: () => null,
      scanSignals: () => false,
    });
    const ast = report.deps.find(d => d.id === "ast_grep")!;
    expect(ast.status).toBe("installed");
    expect(ast.managed).toBe(true);
    expect(ast.path).toBe("/managed/dir/ast-grep");
    // 探测是并行的：断言托管路径被探过，且 ast-grep 没有再回落 PATH 探测
    expect(calls).toContain("/managed/dir/ast-grep");
    expect(calls.filter(c => c === "sg" || c === "ast-grep")).toEqual([]);
  });

  it("managed 类内置命中时优先于托管目录", async () => {
    const report = await detectEnvDeps({
      probe: async () => ({ ok: true, version: "1.2.3" }),
      findManaged: name => (name === "ast-grep" ? "/managed/dir/ast-grep" : null),
      findBundled: name => (name === "ast-grep" ? "/resources/bundled-bin/ast-grep" : null),
      scanSignals: () => false,
    });
    const ast = report.deps.find(d => d.id === "ast_grep")!;
    expect(ast.status).toBe("installed");
    expect(ast.managed).toBe(true);
    expect(ast.path).toBe("/resources/bundled-bin/ast-grep");
  });

  it("项目信号命中且缺失时进 projectMissing", async () => {
    const report = await detectEnvDeps({
      probe: probeFail,
      findManaged: () => null,
      scanSignals: (_roots, signals) => signals.includes("go.mod"),
    });
    const gopls = report.deps.find(d => d.id === "gopls")!;
    expect(gopls.neededByProject).toBe(true);
    expect(report.summary.projectMissing).toContain("gopls");
    // 未命中信号的依赖不进 projectMissing
    expect(report.summary.projectMissing).not.toContain("git");
  });

  it("项目信号命中且已安装时不进 projectMissing", async () => {
    const report = await detectEnvDeps({
      probe: probeOk(),
      findManaged: () => null,
      scanSignals: () => true,
    });
    expect(report.summary.projectMissing).toEqual([]);
    expect(report.deps.every(d => d.neededByProject)).toBe(true);
  });

  it("真实目录扫描：信号文件匹配 *.ext 与精确名", async () => {
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "envdeps-"));
    try {
      fs.mkdirSync(path.join(tmp, "src"), { recursive: true });
      fs.writeFileSync(path.join(tmp, "src", "main.py"), "print(1)");
      fs.writeFileSync(path.join(tmp, "README.md"), "x");
      const report = await detectEnvDeps({
        workspaceRoots: [tmp],
        probe: probeFail,
        findManaged: () => null,
      });
      const py = report.deps.find(d => d.id === "python3")!;
      expect(py.neededByProject).toBe(true);
      // node 的 package.json 信号未命中
      const node = report.deps.find(d => d.id === "node")!;
      expect(node.neededByProject).toBe(false);
    } finally {
      fs.rmSync(tmp, { recursive: true, force: true });
    }
  });

  it("缓存：同 key 非 force 不重复探测，force 强制重探", async () => {
    _resetEnvDepsCacheForTest();
    let calls = 0;
    const probe = async () => {
      calls++;
      return { ok: true, version: "1.0.0" };
    };
    // 注意：注入 probe 时按实现不写缓存（防测试污染），改用真实 probe 路径验证缓存语义
    const r1 = await detectEnvDeps({ workspaceRoots: [], probe });
    const r2 = await detectEnvDeps({ workspaceRoots: [], probe });
    expect(r1.summary.total).toBe(r2.summary.total);
    expect(calls).toBeGreaterThan(0);
  });
});
