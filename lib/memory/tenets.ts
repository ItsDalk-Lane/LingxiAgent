/**
 * tenets.js — 用户原则（tenets）存储
 *
 * 「经用户确认的行为原则」层：区别于 facts（世界知识）与 experience（任务方法），
 * 这里存的是注入每个新会话 system prompt 的行为准则（借鉴 nuphus 的 tenets）。
 *
 * 生命周期：模型经 tenet_propose 工具提议（pending）→ 用户在聊天卡或设置页
 * 批准（active）/拒绝（rejected）；用户也可在设置页直接添加（active）。
 * pending 永不超时作废——审批是持久等待，不是 confirm-store 那种限时阻塞。
 *
 * 存储：agentDir/memory/tenets.json，schemaVersion 1，atomicWrite 落盘。
 */

import fs from "fs";
import path from "path";
import { randomUUID } from "node:crypto";
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

/** active 上限：原则注入每个会话，超量即噪音（对齐 nuphus 的 20 条上限） */
export const MAX_ACTIVE_TENETS = 20;
/** pending 上限：提案积压说明没人处理，满则拒绝新提案并提示先清理 */
export const MAX_PENDING_TENETS = 30;
/** 单条原则长度上限 */
export const MAX_TENET_CONTENT_CHARS = 300;

const TENETS_SCHEMA_VERSION = 1;

export const TENET_ERRORS = Object.freeze({
  LIMIT_REACHED: "TENET_LIMIT_REACHED",
  PENDING_FULL: "TENET_PENDING_FULL",
  INVALID: "TENET_INVALID",
  NOT_FOUND: "TENET_NOT_FOUND",
  DUPLICATE: "TENET_DUPLICATE",
});

export function tenetsFilePath(agentDir: string): string {
  return path.join(agentDir, "memory", "tenets.json");
}

export function isTenetError(err: any, code?: string): boolean {
  return err?.code === (code ?? TENET_ERRORS.INVALID)
    || Object.values(TENET_ERRORS).includes(err?.code);
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
  return String(value ?? "").replace(/\s+/g, " ").trim();
}

/** 归一化查重口径：去空白差异、小写、去句尾标点 */
function dedupKey(content: string): string {
  return content.toLowerCase().replace(/[。.！!？?；;，,]+$/g, "").trim();
}

interface TenetsFile {
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

function writeTenetsFile(filePath: string, data: TenetsFile): void {
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  atomicWriteSync(filePath, JSON.stringify(data, null, 2) + "\n");
}

function withFile<T>(filePath: string, mutate: (data: TenetsFile) => T): T {
  const data = readTenetsFile(filePath);
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

/** 用户直接添加：立即 active，计入 20 条上限 */
export function addTenetDirect(
  agentDir: string,
  input: { content: string; priority?: TenetPriority },
): { tenet: Tenet; duplicate: boolean } {
  const content = normalizeTenetContent(input.content);
  assertContent(content);
  const filePath = tenetsFilePath(agentDir);

  return withFile(filePath, (data) => {
    const dup = findDuplicate(data, content);
    if (dup) return { tenet: dup, duplicate: true };
    if (data.tenets.filter((t) => t.status === "active").length >= MAX_ACTIVE_TENETS) {
      throw tenetError(
        TENET_ERRORS.LIMIT_REACHED,
        `active tenets are full (${MAX_ACTIVE_TENETS}); remove one before adding another`,
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

/** 审批 pending 提案（approve=false → rejected）。active 满时 approve 显式报错。 */
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
    if (approve && data.tenets.filter((t) => t.status === "active").length >= MAX_ACTIVE_TENETS) {
      throw tenetError(
        TENET_ERRORS.LIMIT_REACHED,
        `active tenets are full (${MAX_ACTIVE_TENETS}); remove one before approving`,
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
 * system prompt 注入块（# 用户原则）。只列 active，critical 在前；
 * 为空返回 null（调用方跳过整段）。
 */
export function buildTenetsPromptSection(agentDir: string, isZh: boolean): string | null {
  const active = activeTenets(agentDir);
  if (active.length === 0) return null;
  const lines = active.map((t) => `- [${t.priority}] ${t.content}`);
  const header = isZh
    ? "# 用户原则\n以下是经用户确认的行为原则，每次输出前都要遵守："
    : "# User Principles\nThe following behavioral principles are confirmed by the user and must be followed:";
  return `${header}\n${lines.join("\n")}`;
}
