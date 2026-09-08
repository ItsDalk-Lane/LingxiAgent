/**
 * tenets.js — 置顶与原则（tenets）存储
 *
 * 「钉住的事实 + 经用户确认的行为原则」统一库：区别于 facts（世界知识）与
 * experience（任务方法），这里存的是注入每个新会话 system prompt 的长期约定
 * （原置顶记忆 pins 与用户原则 tenets 已合并，以 tenets 为底座；原 pins 数据
 * 经启动迁移并入，source=user_direct）。
 *
 * 生命周期：模型经 tenet_propose 工具提议（pending）→ 用户在聊天卡或设置页
 * 批准（active）/拒绝（rejected）；用户/模型也可经 pin_memory 工具或设置页
 * 直接添加（active，user_direct）。pending 永不超时作废。
 *
 * 存储：agentDir/memory/tenets.json，schemaVersion 1，atomicWrite 落盘。
 */

import fs from "fs";
import path from "path";
import { createHash, randomUUID } from "node:crypto";
import { atomicWriteSync } from "../../shared/safe-fs.ts";
import { createModuleLogger } from "../debug-log.ts";

const log = createModuleLogger("memory-tenets");

export const TENET_PRIORITIES = Object.freeze(["critical", "high", "medium", "low"] as const);
export type TenetPriority = (typeof TENET_PRIORITIES)[number];

export type TenetStatus = "pending" | "active" | "rejected";
export type TenetSource = "model_proposed" | "user_direct";

export interface Tenet {
  id: string;
  content: string;
  priority: TenetPriority;
  status: TenetStatus;
  source: TenetSource;
  sessionId?: string | null;
  createdAt: string;
  decidedAt?: string | null;
}

/** active/model_proposed 上限（提案审批路径）：原则注入每个会话，超量即噪音 */
export const MAX_ACTIVE_TENETS = 20;
/** active/user_direct 上限（直钉/设置页）：迁移与用户操作是显式意图 */
export const MAX_ACTIVE_USER_TENETS = 200;
/** pending 上限：提案积压说明没人处理，满则拒绝新提案并提示先清理 */
export const MAX_PENDING_TENETS = 30;
/** 单条原则长度上限（普通新增；历史迁移接纳不受此限） */
export const MAX_TENET_CONTENT_CHARS = 300;

const TENETS_SCHEMA_VERSION = 1;

export const TENET_ERRORS = Object.freeze({
  LIMIT_REACHED: "TENET_LIMIT_REACHED",
  PENDING_FULL: "TENET_PENDING_FULL",
  INVALID: "TENET_INVALID",
  NOT_FOUND: "TENET_NOT_FOUND",
  DUPLICATE: "TENET_DUPLICATE",
  STORE_CORRUPTED: "TENET_STORE_CORRUPTED",
  STORE_UNSUPPORTED_SCHEMA: "TENET_STORE_UNSUPPORTED_SCHEMA",
  STORE_READ_FAILED: "TENET_STORE_READ_FAILED",
});

export function tenetsFilePath(agentDir: string): string {
  return path.join(agentDir, "memory", "tenets.json");
}

/**
 * 传 code 时精确匹配该码；不传才表示任意 TenetError。
 * （旧实现传 code 仍匹配任意 tenet 错误码，调用方会把 INVALID 错报成容量满。）
 */
export function isTenetError(err: any, code?: string): boolean {
  if (code !== undefined) return err?.code === code;
  return Object.values(TENET_ERRORS).includes(err?.code);
}

/** active 条目按来源分列计数；pending/rejected 不占 active 配额。 */
export function countActiveTenetsBySource(tenets: readonly Tenet[]): { modelProposed: number; userDirect: number } {
  let modelProposed = 0;
  let userDirect = 0;
  for (const tenet of tenets) {
    if (tenet.status !== "active") continue;
    if (tenet.source === "user_direct") userDirect += 1;
    else modelProposed += 1;
  }
  return { modelProposed, userDirect };
}

function tenetError(code: string, message: string): Error & { code: string } {
  const err: any = new Error(message);
  err.code = code;
  return err;
}

export function normalizeTenetPriority(value: unknown): TenetPriority {
  return TENET_PRIORITIES.includes(value as TenetPriority) ? (value as TenetPriority) : "medium";
}

function normalizeTenetContent(value: unknown): string {
  return String(value ?? "").replace(/\r\n?/g, "\n").trim();
}

/** 归一化查重口径：去空白差异、小写、去句尾标点（导出供迁移预查重） */
export function dedupKey(content: string): string {
  return content.toLowerCase().replace(/[。.！!？?；;，,]+$/g, "").trim();
}

export interface TenetsFile {
  schemaVersion: number;
  tenets: Tenet[];
}

function emptyFile(): TenetsFile {
  return { schemaVersion: TENETS_SCHEMA_VERSION, tenets: [] };
}

export function readTenetsFile(filePath: string): TenetsFile {
  try {
    const raw = JSON.parse(fs.readFileSync(filePath, "utf-8"));
    const tenets = Array.isArray(raw?.tenets)
      ? raw.tenets.filter((t: any) => t && typeof t.content === "string" && t.content.trim())
      : [];
    return {
      schemaVersion: Number(raw?.schemaVersion) || TENETS_SCHEMA_VERSION,
      tenets: tenets.map(normalizeTenet),
    };
  } catch {
    return emptyFile();
  }
}

/**
 * 写边界的严格读取：只有 ENOENT 可以按空库创建；损坏 JSON、顶层形状非法、
 * 未来 schemaVersion、EACCES 等读取失败都明确抛错——写入路径（withFile 与
 * 迁移）不得在目标损坏时读成空库后覆盖。读取路径（listTenets 等 UI 展示）
 * 仍沿用 readTenetsFile 的降级语义，不在本调整范围内。
 */
export function readTenetsFileStrict(filePath: string): TenetsFile {
  let rawText: string;
  try {
    rawText = fs.readFileSync(filePath, "utf-8");
  } catch (err: any) {
    if (err?.code === "ENOENT") return emptyFile();
    throw tenetError(
      TENET_ERRORS.STORE_READ_FAILED,
      `cannot read tenets store: ${err?.code || err?.message || err}`,
    );
  }
  let raw: any;
  try {
    raw = JSON.parse(rawText);
  } catch {
    throw tenetError(TENET_ERRORS.STORE_CORRUPTED, "tenets store is not valid JSON");
  }
  if (raw === null || typeof raw !== "object" || Array.isArray(raw)) {
    throw tenetError(TENET_ERRORS.STORE_CORRUPTED, "tenets store has an invalid top-level shape");
  }
  const schemaVersion = Number(raw.schemaVersion) || TENETS_SCHEMA_VERSION;
  if (schemaVersion > TENETS_SCHEMA_VERSION) {
    throw tenetError(
      TENET_ERRORS.STORE_UNSUPPORTED_SCHEMA,
      `tenets store schemaVersion ${schemaVersion} is newer than supported ${TENETS_SCHEMA_VERSION}`,
    );
  }
  if (raw.tenets !== undefined && !Array.isArray(raw.tenets)) {
    throw tenetError(TENET_ERRORS.STORE_CORRUPTED, "tenets store field 'tenets' is not an array");
  }
  const tenets = (raw.tenets ?? []).filter((t: any) => t && typeof t.content === "string" && t.content.trim());
  return { schemaVersion, tenets: tenets.map(normalizeTenet) };
}

function normalizeTenet(raw: any): Tenet {
  return {
    id: typeof raw.id === "string" && raw.id ? raw.id : randomUUID(),
    content: normalizeTenetContent(raw.content),
    priority: normalizeTenetPriority(raw.priority),
    status: raw.status === "active" || raw.status === "rejected" ? raw.status : "pending",
    source: raw.source === "user_direct" ? "user_direct" : "model_proposed",
    sessionId: typeof raw.sessionId === "string" && raw.sessionId ? raw.sessionId : null,
    createdAt: typeof raw.createdAt === "string" && raw.createdAt ? raw.createdAt : new Date().toISOString(),
    decidedAt: typeof raw.decidedAt === "string" && raw.decidedAt ? raw.decidedAt : null,
  };
}

/** 确定性序列化（迁移靠它预计算目标字节与结果哈希；写文件与验证共用同一配方） */
export function serializeTenetsFile(data: TenetsFile): string {
  return JSON.stringify(data, null, 2) + "\n";
}

function writeTenetsFile(filePath: string, data: TenetsFile): void {
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  atomicWriteSync(filePath, serializeTenetsFile(data));
}

function withFile<T>(filePath: string, mutate: (data: TenetsFile) => T): T {
  const data = readTenetsFileStrict(filePath);
  const result = mutate(data);
  writeTenetsFile(filePath, data);
  return result;
}

function assertContent(content: string): void {
  if (!content) {
    throw tenetError(TENET_ERRORS.INVALID, "tenet content must be a non-empty string");
  }
  if (content.length > MAX_TENET_CONTENT_CHARS) {
    throw tenetError(
      TENET_ERRORS.INVALID,
      `tenet content exceeds ${MAX_TENET_CONTENT_CHARS} chars (got ${content.length})`,
    );
  }
}

function findDuplicate(data: TenetsFile, content: string): Tenet | null {
  const key = dedupKey(content);
  return data.tenets.find((t) => dedupKey(t.content) === key) || null;
}

/** 全量列表（按状态分组前排序：priority 升序=critical 在前，再按创建时间） */
export function listTenets(agentDir: string): Tenet[] {
  const data = readTenetsFile(tenetsFilePath(agentDir));
  const weight: Record<TenetPriority, number> = { critical: 0, high: 1, medium: 2, low: 3 };
  return [...data.tenets].sort((a, b) => (
    weight[a.priority] - weight[b.priority]
    || a.createdAt.localeCompare(b.createdAt)
  ));
}

export function activeTenets(agentDir: string): Tenet[] {
  return listTenets(agentDir).filter((t) => t.status === "active");
}

export function pendingTenets(agentDir: string): Tenet[] {
  return listTenets(agentDir).filter((t) => t.status === "pending");
}

/**
 * 模型提案：写入 pending。与任何既有条目（任意状态）归一化重复时返回
 * { duplicate, existing }，不重复写库。
 */
export function addTenetProposal(
  agentDir: string,
  input: { content: string; priority?: TenetPriority; sessionId?: string | null },
): { tenet: Tenet; duplicate: boolean; existingStatus?: TenetStatus } {
  const content = normalizeTenetContent(input.content);
  assertContent(content);
  const filePath = tenetsFilePath(agentDir);

  return withFile(filePath, (data) => {
    const dup = findDuplicate(data, content);
    if (dup) {
      return { tenet: dup, duplicate: true, existingStatus: dup.status };
    }
    if (data.tenets.filter((t) => t.status === "pending").length >= MAX_PENDING_TENETS) {
      throw tenetError(
        TENET_ERRORS.PENDING_FULL,
        `pending tenets are full (${MAX_PENDING_TENETS}); ask the user to review pending proposals first`,
      );
    }
    const tenet: Tenet = {
      id: randomUUID(),
      content,
      priority: normalizeTenetPriority(input.priority),
      status: "pending",
      source: "model_proposed",
      sessionId: typeof input.sessionId === "string" && input.sessionId ? input.sessionId : null,
      createdAt: new Date().toISOString(),
      decidedAt: null,
    };
    data.tenets.push(tenet);
    return { tenet, duplicate: false };
  });
}

/**
 * 用户直接添加：立即 active/user_direct，配额只计 active/user_direct（与
 * model_proposed 提案配额互不占用）。与既有 active 条目归一化重复时返回
 * duplicate 不新增；只与 pending/rejected 重复时仍产生新的 active 条目
 * （用户显式直钉必须生效），历史提案状态不被篡改。
 */
export function addTenetDirect(
  agentDir: string,
  input: { content: string; priority?: TenetPriority },
): { tenet: Tenet; duplicate: boolean } {
  const content = normalizeTenetContent(input.content);
  assertContent(content);
  const filePath = tenetsFilePath(agentDir);

  return withFile(filePath, (data) => {
    const key = dedupKey(content);
    const activeDup = data.tenets.find((t) => t.status === "active" && dedupKey(t.content) === key);
    if (activeDup) return { tenet: activeDup, duplicate: true };
    if (countActiveTenetsBySource(data.tenets).userDirect >= MAX_ACTIVE_USER_TENETS) {
      throw tenetError(
        TENET_ERRORS.LIMIT_REACHED,
        `active user-pinned tenets are full (${MAX_ACTIVE_USER_TENETS}); remove one before adding another`,
      );
    }
    const tenet: Tenet = {
      id: randomUUID(),
      content,
      priority: normalizeTenetPriority(input.priority),
      status: "active",
      source: "user_direct",
      sessionId: null,
      createdAt: new Date().toISOString(),
      decidedAt: new Date().toISOString(),
    };
    data.tenets.push(tenet);
    return { tenet, duplicate: false };
  });
}

/** 审批 pending 提案（approve=false → rejected）。active/model_proposed 满时 approve 显式报错。 */
export function decideTenet(agentDir: string, tenetId: string, approve: boolean): Tenet {
  const filePath = tenetsFilePath(agentDir);
  return withFile(filePath, (data) => {
    const tenet = data.tenets.find((t) => t.id === tenetId);
    if (!tenet) {
      throw tenetError(TENET_ERRORS.NOT_FOUND, `tenet ${tenetId} not found`);
    }
    if (tenet.status !== "pending") {
      throw tenetError(TENET_ERRORS.INVALID, `tenet ${tenetId} is already ${tenet.status}`);
    }
    if (approve && countActiveTenetsBySource(data.tenets).modelProposed >= MAX_ACTIVE_TENETS) {
      throw tenetError(
        TENET_ERRORS.LIMIT_REACHED,
        `active model-proposed tenets are full (${MAX_ACTIVE_TENETS}); remove one before approving`,
      );
    }
    tenet.status = approve ? "active" : "rejected";
    tenet.decidedAt = new Date().toISOString();
    log.log(`tenet ${tenetId} → ${tenet.status}`);
    return tenet;
  });
}

export function removeTenet(agentDir: string, tenetId: string): boolean {
  const filePath = tenetsFilePath(agentDir);
  return withFile(filePath, (data) => {
    const before = data.tenets.length;
    data.tenets = data.tenets.filter((t) => t.id !== tenetId);
    return data.tenets.length < before;
  });
}

/**
 * system prompt 注入块（# 置顶与原则）。只列 active，critical 在前；
 * 多行内容按缩进续行渲染（沿用原 pinned.md 的列表续行约定）；
 * 为空返回 null（调用方跳过整段）。
 */
export function buildTenetsPromptSection(agentDir: string, isZh: boolean): string | null {
  const active = activeTenets(agentDir);
  if (active.length === 0) return null;
  const lines = active.flatMap((t) => {
    const contentLines = t.content.split("\n");
    return contentLines.map((line, index) => index === 0 ? `- ${line}` : `  ${line}`);
  });
  const header = isZh
    ? "# 置顶与原则\n以下是用户钉住的内容与经用户确认的行为原则，始终遵守："
    : "# Pinned Items & Principles\nThe following pinned content and user-confirmed behavioral principles always apply:";
  return `${header}\n${lines.join("\n")}`;
}

// ── 历史迁移批量接纳（仅限内部迁移/恢复入口调用，不暴露给模型、HTTP 或设置页） ──

/** 迁移去重口径：换行归一 + 边界空白归一后的精确内容比较（不做小写/去标点等更宽合并） */
export function legacyContentKey(content: string): string {
  return normalizeTenetContent(content);
}

export interface LegacyPinImportItem {
  /** 旧 pin 的原始 id（只记录在迁移映射/收据里，不会变成新 tenet 的 id） */
  legacyId: string | null;
  content: string;
  /** 合法（可解析）的创建时间保留；非法/缺失用当前时间 */
  createdAt?: string | null;
}

export type LegacyImportOutcome =
  | "added"
  | "added_over_pending_history"
  | "added_over_rejected_history"
  | "duplicate_active"
  | "duplicate_in_batch";

export interface LegacyPinImportEntry {
  order: number;
  legacyId: string | null;
  /** sha256:<hex>，按归一化内容计算（收据只记哈希，不记正文） */
  contentHash: string;
  normalizedContent: string;
  outcome: LegacyImportOutcome;
  tenetId: string;
  createdAt: string;
  decidedAt: string;
  /** 与 pending/rejected 历史条目精确重复时，记录被保留的历史条目 id */
  historyTenetId?: string | null;
}

/** 恢复重跑时采用收据中已规划的 id/时间戳，保证同一计划两次提交产生逐字节一致的目标文件 */
export interface LegacyImportPlanOverride {
  tenetId: string;
  createdAt: string;
  decidedAt: string;
}

function legacyImportContentHash(normalizedContent: string): string {
  return `sha256:${createHash("sha256").update(normalizedContent, "utf-8").digest("hex")}`;
}

function validLegacyCreatedAt(value: unknown): string | null {
  if (typeof value !== "string" || !value) return null;
  const ms = Date.parse(value);
  return Number.isNaN(ms) ? null : new Date(ms).toISOString();
}

/**
 * 纯规划函数：对一个目标库快照计算全部候选、冲突与最终状态。
 * 迁移用它预计算最终字节与结果哈希；importLegacyPinnedItems 用它在单次
 * withFile 内原子落盘。豁免普通新增的 300 字符与 200 条上限（豁免原因
 * legacy migration 由调用方记进收据），但仍要求归一化后非空。
 */
export function planLegacyPinnedImport(
  data: TenetsFile,
  items: readonly LegacyPinImportItem[],
  overrides?: readonly (LegacyImportPlanOverride | undefined)[],
): { entries: LegacyPinImportEntry[]; finalTenets: Tenet[] } {
  const finalTenets = [...data.tenets];
  const entries: LegacyPinImportEntry[] = [];
  const batchNewByKey = new Map<string, Tenet>();

  items.forEach((item, order) => {
    const normalizedContent = legacyContentKey(item.content);
    const contentHash = legacyImportContentHash(normalizedContent);
    const override = overrides?.[order];

    const existingExact = finalTenets.find((t) => legacyContentKey(t.content) === normalizedContent);
    if (existingExact && existingExact.status === "active") {
      entries.push({
        order,
        legacyId: item.legacyId ?? null,
        contentHash,
        normalizedContent,
        outcome: "duplicate_active",
        tenetId: existingExact.id,
        createdAt: existingExact.createdAt,
        decidedAt: existingExact.decidedAt ?? existingExact.createdAt,
      });
      return;
    }

    const batchDup = batchNewByKey.get(normalizedContent);
    if (batchDup) {
      entries.push({
        order,
        legacyId: item.legacyId ?? null,
        contentHash,
        normalizedContent,
        outcome: "duplicate_in_batch",
        tenetId: batchDup.id,
        createdAt: batchDup.createdAt,
        decidedAt: batchDup.decidedAt ?? batchDup.createdAt,
      });
      return;
    }

    const historyTenet = existingExact && existingExact.status !== "active" ? existingExact : null;
    const createdAt = override?.createdAt ?? validLegacyCreatedAt(item.createdAt) ?? new Date().toISOString();
    const decidedAt = override?.decidedAt ?? createdAt;
    const tenet: Tenet = {
      id: override?.tenetId ?? randomUUID(),
      content: normalizedContent,
      priority: "high",
      status: "active",
      source: "user_direct",
      sessionId: null,
      createdAt,
      decidedAt,
    };
    finalTenets.push(tenet);
    batchNewByKey.set(normalizedContent, tenet);
    entries.push({
      order,
      legacyId: item.legacyId ?? null,
      contentHash,
      normalizedContent,
      outcome: historyTenet
        ? (historyTenet.status === "pending" ? "added_over_pending_history" : "added_over_rejected_history")
        : "added",
      tenetId: tenet.id,
      createdAt: tenet.createdAt,
      decidedAt: tenet.decidedAt ?? tenet.createdAt,
      historyTenetId: historyTenet?.id ?? null,
    });
  });

  return { entries, finalTenets };
}

/**
 * 历史迁移批量接纳入口：一个 agent 一次原子写入。仅供迁移与恢复工具内部
 * 调用——不注册为工具、不挂 HTTP、不接设置页；普通请求无法经此绕过限制。
 */
export function importLegacyPinnedItems(
  agentDir: string,
  items: readonly LegacyPinImportItem[],
): { entries: LegacyPinImportEntry[] } {
  const filePath = tenetsFilePath(agentDir);
  return withFile(filePath, (data) => {
    const { entries, finalTenets } = planLegacyPinnedImport(data, items);
    data.tenets = finalTenets;
    return { entries };
  });
}
