/**
 * core/migrations.ts 单元测试
 *
 * #1–#53 旧版本存量数据迁移随"全新安装、无遗留数据"前提整体退役；
 * #54（清理 utility_model / utility_large_model 死键）起以真实迁移作为
 * runner 机制的夹具。新增迁移时，把该迁移的行为测试加在本文件，并同步
 * registryLatestId 断言。
 */
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import { getMigrationStatus, runMigrations } from "../core/migrations.ts";

// ── 测试工具 ────────────────────────────────────────────────────────────────

function makeTmpDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "hana-migrations-"));
}

/** 最小化 PreferencesManager stub */
function makePrefs(userDir) {
  const p = path.join(userDir, "preferences.json");
  fs.mkdirSync(userDir, { recursive: true });
  if (!fs.existsSync(p)) fs.writeFileSync(p, "{}", "utf-8");
  return {
    getPreferences() { return JSON.parse(fs.readFileSync(p, "utf-8")); },
    savePreferences(data) {
      fs.writeFileSync(p, JSON.stringify(data, null, 2) + "\n", "utf-8");
    },
  };
}

function readPrefs(userDir) {
  return JSON.parse(fs.readFileSync(path.join(userDir, "preferences.json"), "utf-8"));
}

// ── runner 行为 ──────────────────────────────────────────────────────────────

describe("runMigrations runner", () => {
  let tmpDir, agentsDir, userDir;

  beforeEach(() => {
    tmpDir = makeTmpDir();
    agentsDir = path.join(tmpDir, "agents");
    userDir = path.join(tmpDir, "user");
    fs.mkdirSync(agentsDir, { recursive: true });
  });

  afterEach(() => { fs.rmSync(tmpDir, { recursive: true, force: true }); });

  it("高水位已达 registryLatest：不执行任何迁移，也不写盘", () => {
    const prefs = makePrefs(userDir);
    prefs.savePreferences({ _dataVersion: 54, keep: "unchanged" });
    const before = fs.readFileSync(path.join(userDir, "preferences.json"), "utf-8");

    const status = runMigrations({
      lingxiHome: tmpDir,
      agentsDir,
      prefs,
      providerRegistry: null,
      log: () => {},
    });

    expect(status).toEqual({
      registryLatestId: 54,
      pendingIds: [],
      lastFailedIds: [],
    });
    // 没有待执行迁移时 runner 提前返回，连收据都不写。
    expect(fs.readFileSync(path.join(userDir, "preferences.json"), "utf-8")).toBe(before);
  });

  it("兼容只有 _dataVersion 的旧偏好，并且状态查询不写盘", () => {
    const prefs = makePrefs(userDir);
    prefs.savePreferences({ _dataVersion: 47, keep: "unchanged" });
    const before = fs.readFileSync(path.join(userDir, "preferences.json"), "utf-8");

    expect(getMigrationStatus(prefs)).toEqual({
      registryLatestId: 54,
      pendingIds: [54],
      lastFailedIds: [],
    });
    expect(fs.readFileSync(path.join(userDir, "preferences.json"), "utf-8")).toBe(before);
  });

  it("收据里引用已退役编号的 completedIds / lastFailedIds 会被过滤", () => {
    const prefs = makePrefs(userDir);
    prefs.savePreferences({
      _dataVersion: 53,
      _migrationState: { completedIds: [51, 52], lastFailedIds: [49] },
    });

    // 已退役编号不在注册表内，不能当作有效收据重放；#54 高于高水位 53，待执行。
    expect(getMigrationStatus(prefs)).toEqual({
      registryLatestId: 54,
      pendingIds: [54],
      lastFailedIds: [],
    });
  });

  it("getMigrationStatus 接受裸 preferences 对象", () => {
    expect(getMigrationStatus({ _dataVersion: 54 })).toEqual({
      registryLatestId: 54,
      pendingIds: [],
      lastFailedIds: [],
    });
    expect(getMigrationStatus(null)).toEqual({
      registryLatestId: 54,
      pendingIds: [54],
      lastFailedIds: [],
    });
  });
});

// ── #54 清理 utility_model / utility_large_model 死键 ────────────────────────

describe("#54 migrateRemoveLegacyUtilityModelKeys", () => {
  let tmpDir, agentsDir, userDir;

  beforeEach(() => {
    tmpDir = makeTmpDir();
    agentsDir = path.join(tmpDir, "agents");
    userDir = path.join(tmpDir, "user");
    fs.mkdirSync(agentsDir, { recursive: true });
  });

  afterEach(() => { fs.rmSync(tmpDir, { recursive: true, force: true }); });

  it("删除两个死键，其余偏好原样保留", () => {
    const prefs = makePrefs(userDir);
    prefs.savePreferences({
      _dataVersion: 53,
      utility_model: { id: "gemma4:12b-nvfp4", provider: "ollama" },
      utility_large_model: { id: "gemma4:31b-nvfp4", provider: "ollama" },
      memory_model: { id: "keep-me", provider: "ollama" },
      locale: "zh",
    });

    const status = runMigrations({ lingxiHome: tmpDir, agentsDir, prefs, providerRegistry: null, log: () => {} });

    const after = readPrefs(userDir);
    expect(after.utility_model).toBeUndefined();
    expect(after.utility_large_model).toBeUndefined();
    // 活跃 Slot 键与其余偏好不受影响。
    expect(after.memory_model).toEqual({ id: "keep-me", provider: "ollama" });
    expect(after.locale).toBe("zh");
    // 收据推进到 54。
    expect(after._dataVersion).toBe(54);
    expect(status).toEqual({ registryLatestId: 54, pendingIds: [], lastFailedIds: [] });
  });

  it("幂等：对已迁移状态重跑断言零数据变更", () => {
    const prefs = makePrefs(userDir);
    prefs.savePreferences({
      _dataVersion: 54,
      memory_model: { id: "keep-me", provider: "ollama" },
    });

    const status = runMigrations({ lingxiHome: tmpDir, agentsDir, prefs, providerRegistry: null, log: () => {} });
    const after = readPrefs(userDir);

    expect(status.pendingIds).toEqual([]);
    expect(after).toEqual({
      _dataVersion: 54,
      memory_model: { id: "keep-me", provider: "ollama" },
    });
  });

  it("全新安装（无死键）：#54 空跑后只落收据，不写入任何业务键", () => {
    const prefs = makePrefs(userDir);

    const status = runMigrations({ lingxiHome: tmpDir, agentsDir, prefs, providerRegistry: null, log: () => {} });
    const after = readPrefs(userDir);

    expect(status.pendingIds).toEqual([]);
    expect(after.utility_model).toBeUndefined();
    expect(after.utility_large_model).toBeUndefined();
    expect(Object.keys(after).filter((k) => !k.startsWith("_"))).toEqual([]);
  });
});
