/**
 * 数据迁移 runner
 *
 * 所有用户数据格式变更集中在此文件。
 * preferences.json._dataVersion 保留为连续完成的高水位。
 * 高水位之后的成功条目会单独记录，不会因为前面某条失败而重跑。
 *
 * 添加新迁移：在 migrations 对象末尾加一条，key 为递增整数。
 *
 * 跨分支合并规约：若合并双方在各自分支上都新增过迁移编号（冲突通常落在本文件
 * 或测试里的 LATEST_DATA_VERSION 上，那就是触发信号），禁止裸改号或折叠合入
 * 既有编号——"编号 ≤ 高水位即跳过"意味着任何 ≤ 对方高水位的槽位对对方存量
 * 用户永远不可达，改号救不了；预留高段位同样错误（高号会把高水位推顶，反向
 * 跳过另一侧后续迁移）。唯一正确做法：新增一条更高位的幂等 reconcile 迁移，
 * 依次重放双方全部新增载荷；各载荷的幂等性必须有测试背书（对已迁移状态重跑
 * 断言零变更），并补双方历史高水位的组合测试。
 */
import { createModuleLogger } from "../lib/debug-log.ts";

const moduleLog = createModuleLogger("migrations");

// ── 迁移表 ──────────────────────────────────────────────────────────────────

// 已发布的最高迁移编号是 54（#1–#53 为旧版本存量数据迁移，随"全新安装、无遗留
// 数据"前提整体退役；#54 起为退役后的新迁移）。新迁移编号必须严格递增，禁止复用
// 已发布编号——任何拿到旧安装包的用户 preferences.json 里 _dataVersion 可能已经
// 达到该编号，"编号 ≤ 高水位即跳过"。

/**
 * #54 — 清理语义 Slot 重构后残留的 utility_model / utility_large_model 死键。
 *
 * 283d9581（辅助模型语义 Slot 重构）删除了两个旧键的全部读写路径，但存量
 * preferences.json 里的值一直没清。死键会让用户误以为「取标题/记忆模型已配置」
 * （实际读取侧走 title_model/memory_model 等 Slot 键，未配置回退 chat）。
 * 纯删除、不做值迁移：Slot 体系默认回退 chat 已是一个月来的实际行为，
 * 静默把旧值映射到某个 Slot 反而会凭空切换用户正在用的模型。
 */
function migrateRemoveLegacyUtilityModelKeys(ctx) {
  const { prefs, log } = ctx;
  const preferences = prefs.getPreferences();

  let changed = false;
  for (const key of ["utility_model", "utility_large_model"]) {
    if (!Object.prototype.hasOwnProperty.call(preferences, key)) continue;
    delete preferences[key];
    changed = true;
    log(`[migrations] #54 preferences.${key} 已删除（语义 Slot 重构后无读取方的死键）`);
  }
  if (changed) prefs.savePreferences(preferences);
}

const migrations = {
  54: migrateRemoveLegacyUtilityModelKeys,
};

// Migration ids are a single monotonic ladder shared across release channels;
// a new id must exceed the highest id ever shipped on ANY channel, because the
// runner treats id <= highWaterMark as completed.
const migrationDependencies = {};

const migrationIds = Object.keys(migrations).map(Number).sort((a, b) => a - b);
const latestMigrationId = migrationIds.at(-1) || 0;

function normalizeMigrationState(preferences) {
  const highWaterMark = Number.isInteger(preferences?._dataVersion) && preferences._dataVersion > 0
    ? preferences._dataVersion
    : 0;
  const rawState = preferences?._migrationState;
  const completedIds: number[] = Array.isArray(rawState?.completedIds)
    ? rawState.completedIds.filter((id) => Number.isInteger(id) && migrationIds.includes(id) && id > highWaterMark)
    : [];
  const lastFailedIds: number[] = Array.isArray(rawState?.lastFailedIds)
    ? rawState.lastFailedIds.filter((id) => Number.isInteger(id) && migrationIds.includes(id) && id > highWaterMark)
    : [];
  return {
    highWaterMark,
    completedIds: [...new Set(completedIds)].sort((a, b) => a - b),
    lastFailedIds: [...new Set(lastFailedIds)].sort((a, b) => a - b),
  };
}

function completedMigrationIds(state) {
  const completed = new Set(state.completedIds);
  for (const id of migrationIds) {
    if (id <= state.highWaterMark) completed.add(id);
  }
  return completed;
}

function compactMigrationState(state, completed) {
  let highWaterMark = state.highWaterMark;
  for (const id of migrationIds) {
    if (id <= highWaterMark) continue;
    if (id !== highWaterMark + 1 || !completed.has(id)) break;
    highWaterMark = id;
  }
  return {
    highWaterMark,
    completedIds: [...completed].filter((id) => id > highWaterMark).sort((a, b) => a - b),
    lastFailedIds: state.lastFailedIds.filter((id) => id > highWaterMark).sort((a, b) => a - b),
  };
}

function saveMigrationState(prefs, state) {
  const fresh = prefs.getPreferences();
  fresh._dataVersion = state.highWaterMark;
  fresh._migrationState = {
    completedIds: state.completedIds,
    lastFailedIds: state.lastFailedIds,
  };
  prefs.savePreferences(fresh);
}

/**
 * Returns legacy migration readiness without changing preferences or user data.
 * Accepts either a PreferencesManager-like object or an already-read preferences object.
 */
export function getMigrationStatus(prefsOrPreferences) {
  const preferences = typeof prefsOrPreferences?.getPreferences === "function"
    ? prefsOrPreferences.getPreferences()
    : (prefsOrPreferences || {});
  const state = normalizeMigrationState(preferences);
  const completed = completedMigrationIds(state);
  return {
    registryLatestId: latestMigrationId,
    pendingIds: migrationIds.filter((id) => !completed.has(id)),
    lastFailedIds: state.lastFailedIds.filter((id) => !completed.has(id)),
  };
}

// ── Runner ──────────────────────────────────────────────────────────────────

/**
 * @param {object} ctx
 * @param {string}   ctx.lingxiHome
 * @param {string}   ctx.agentsDir
 * @param {import('./preferences-manager.ts').PreferencesManager} ctx.prefs
 * @param {import('./provider-registry.ts').ProviderRegistry}     ctx.providerRegistry
 * @param {Function} ctx.log
 */
export function runMigrations(ctx) {
  const { prefs, log } = ctx;
  const preferences = prefs.getPreferences();
  let state = normalizeMigrationState(preferences);
  const completed = completedMigrationIds(state);
  const pending = migrationIds.filter((id) => !completed.has(id));

  if (!pending.length) return getMigrationStatus(prefs);

  log(`[migrations] _dataVersion=${state.highWaterMark}，待执行 ${pending.length} 条迁移`);

  for (const v of pending) {
    const unmetDependencies = (migrationDependencies[v] || []).filter((id) => !completed.has(id));
    if (unmetDependencies.length > 0) {
      log(`[migrations] #${v} 等待前置迁移 #${unmetDependencies.join(", #")}`);
      continue;
    }

    try {
      migrations[v](ctx);
      log(`[migrations] #${v} 完成`);
      completed.add(v);
      state.lastFailedIds = state.lastFailedIds.filter((id) => id !== v);
    } catch (err) {
      moduleLog.error(`#${v} 失败: ${err.message}`);
      if (!state.lastFailedIds.includes(v)) state.lastFailedIds.push(v);
    }

    // 每次尝试后立即持久化收据，防止后续崩溃导致重跑已成功的迁移。
    state = compactMigrationState(state, completed);
    try {
      saveMigrationState(prefs, state);
    } catch (err) {
      // The migration's own result and the receipt write are separate
      // failure domains. A read-only disk or a transient atomic-rename
      // failure must not turn maintenance bookkeeping into a global startup
      // failure. Without a durable receipt the successful migration remains
      // pending and will be retried on the next launch.
      moduleLog.error(`迁移收据保存失败: ${err.message}`);
      log(`[migrations] 收据保存失败，应用将继续启动；未落盘的迁移会在下次启动重试`);
    }
  }

  return getMigrationStatus(prefs);
}
