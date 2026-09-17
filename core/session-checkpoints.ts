/**
 * session-checkpoints.ts — 会话具名存档点（阶段二·8）。
 *
 * 存档落会话旁侧车 `${sessionPath}.checkpoints.json`（模式照 workspace
 * 快照侧车），不进消息流：重载/分页/压缩都不受影响，删了也只是丢存档点。
 * 记录 = { name, target:{role,entryId}, turnInputEntryId, snapshotCommit,
 * createdAt, messageCount }。名字唯一：`latest` 是保留占位名（重复 create
 * 覆盖——它语义就是「最近的」），其他名字冲突拒绝、绝不静默覆盖。
 * 快照 commit 只是引用（影子仓库自有账本），不在此处复制数据。
 */
import fs from "node:fs";
import path from "node:path";

export const SESSION_CHECKPOINT_RESERVED_NAME = "latest";
/** 侧车记录上限（lazy 窗口化：追加时裁最老的非 latest 记录）。 */
export const SESSION_CHECKPOINT_MAX_RECORDS = 200;

export function checkpointSidecarPath(sessionPath: string): string {
  return `${sessionPath}.checkpoints.json`;
}

export interface SessionCheckpointRecord {
  schemaVersion: 1;
  name: string;
  target: { role: string; entryId: string };
  turnInputEntryId: string | null;
  snapshotCommit: string | null;
  snapshotDegraded: boolean;
  createdAt: number;
  messageCount: number;
}

function readSidecar(sidecarPath: string): { records: SessionCheckpointRecord[] } {
  try {
    const raw = JSON.parse(fs.readFileSync(sidecarPath, "utf8"));
    const records = Array.isArray(raw?.records) ? raw.records.filter((r: any) => (
      r && typeof r === "object" && typeof r.name === "string" && r.name
      && r?.target?.entryId
    )) : [];
    return { records };
  } catch {
    return { records: [] };
  }
}

function writeSidecar(sidecarPath: string, records: SessionCheckpointRecord[]): void {
  const tmp = `${sidecarPath}.tmp-${process.pid}-${Date.now()}`;
  fs.mkdirSync(path.dirname(sidecarPath), { recursive: true });
  fs.writeFileSync(tmp, JSON.stringify({ schemaVersion: 1, records }, null, 2));
  fs.renameSync(tmp, sidecarPath);
}

export function listSessionCheckpoints(sessionPath: string): SessionCheckpointRecord[] {
  return readSidecar(checkpointSidecarPath(sessionPath)).records;
}

export function getSessionCheckpoint(sessionPath: string, name: string): SessionCheckpointRecord | null {
  return readSidecar(checkpointSidecarPath(sessionPath)).records.find((r) => r.name === name) || null;
}

export interface UpsertSessionCheckpointInput {
  name: string;
  target: { role: string; entryId: string };
  turnInputEntryId: string | null;
  snapshotCommit?: string | null;
  snapshotDegraded?: boolean;
  messageCount?: number;
}

/**
 * 写存档点。`latest` 覆盖旧值；其他名字已存在 → 抛错（调用方如实转达，
 * 不静默覆盖）。写入前 lazy 窗口化：超出上限裁最老的非 latest 记录。
 */
export function upsertSessionCheckpoint(sessionPath: string, input: UpsertSessionCheckpointInput): SessionCheckpointRecord {
  const name = typeof input?.name === "string" ? input.name.trim() : "";
  if (!name) throw new Error("checkpoint name is required");
  const sidecarPath = checkpointSidecarPath(sessionPath);
  const { records } = readSidecar(sidecarPath);
  const existing = records.find((r) => r.name === name);
  if (existing && name !== SESSION_CHECKPOINT_RESERVED_NAME) {
    throw new Error(`checkpoint "${name}" already exists — pick a different name (drop it first if you mean to replace it)`);
  }
  const record: SessionCheckpointRecord = {
    schemaVersion: 1,
    name,
    target: { role: input.target?.role || "user", entryId: input.target?.entryId },
    turnInputEntryId: input.turnInputEntryId ?? null,
    snapshotCommit: input.snapshotCommit ?? null,
    snapshotDegraded: input.snapshotDegraded === true,
    createdAt: Date.now(),
    messageCount: Number.isInteger(input.messageCount) ? (input.messageCount as number) : 0,
  };
  const next = existing
    ? records.map((r) => (r.name === name ? record : r))
    : [...records, record];
  // lazy 窗口化：保 latest，裁最老的其他记录
  while (next.length > SESSION_CHECKPOINT_MAX_RECORDS) {
    const idx = next.findIndex((r) => r.name !== SESSION_CHECKPOINT_RESERVED_NAME);
    if (idx === -1) break;
    next.splice(idx, 1);
  }
  writeSidecar(sidecarPath, next);
  return record;
}

export function dropSessionCheckpoint(sessionPath: string, name: string): boolean {
  const sidecarPath = checkpointSidecarPath(sessionPath);
  const { records } = readSidecar(sidecarPath);
  const next = records.filter((r) => r.name !== name);
  if (next.length === records.length) return false;
  writeSidecar(sidecarPath, next);
  return true;
}
