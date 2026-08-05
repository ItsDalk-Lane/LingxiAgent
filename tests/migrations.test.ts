/**
 * core/migrations.ts 单元测试
 *
 * 旧版本存量数据迁移（#1–#53）随"全新安装、无遗留数据"前提整体退役，
 * 迁移注册表当前为空。这里只保留管线基础设施（runMigrations /
 * getMigrationStatus / 收据状态机）的行为测试；未来新增迁移时，
 * 把该迁移的行为测试加回本文件，并以真实迁移作为 runner 机制的夹具。
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

  it("空注册表 + 全新偏好：不执行任何迁移，也不写盘", () => {
    const prefs = makePrefs(userDir);
    const before = fs.readFileSync(path.join(userDir, "preferences.json"), "utf-8");

    const status = runMigrations({
      lingxiHome: tmpDir,
      agentsDir,
      prefs,
      providerRegistry: null,
      log: () => {},
    });

    expect(status).toEqual({
      registryLatestId: 0,
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
      registryLatestId: 0,
      pendingIds: [],
      lastFailedIds: [],
    });
    expect(fs.readFileSync(path.join(userDir, "preferences.json"), "utf-8")).toBe(before);
  });

  it("收据里引用已退役编号的 completedIds / lastFailedIds 会被过滤", () => {
    const prefs = makePrefs(userDir);
    prefs.savePreferences({
      _migrationState: { completedIds: [51, 52], lastFailedIds: [49] },
    });

    // 已退役编号不在注册表内，不能当作有效收据重放。
    expect(getMigrationStatus(prefs)).toEqual({
      registryLatestId: 0,
      pendingIds: [],
      lastFailedIds: [],
    });
  });

  it("getMigrationStatus 接受裸 preferences 对象", () => {
    expect(getMigrationStatus({ _dataVersion: 53 })).toEqual({
      registryLatestId: 0,
      pendingIds: [],
      lastFailedIds: [],
    });
    expect(getMigrationStatus(null)).toEqual({
      registryLatestId: 0,
      pendingIds: [],
      lastFailedIds: [],
    });
  });
});
