/**
 * pinned-tenets-recovery.ts — 旧迁移遗留 .migrated 数据的仅预览恢复入口。
 *
 * 适用条件：agent 目录里存在 pinned.md.migrated* / pinned-memory.json.migrated*
 * 归档文件，且没有 state=completed 的新版迁移收据（completed 说明迁移已收口，
 * 之后缺失的条目视为用户主动删除，本工具不复活）。
 *
 * 默认行为是 dry-run（scanPinnedTenetsRecovery）：列出归档源、当前库里的
 * present/inactive/similar/missing 分类与可恢复数量，不改任何文件。
 * 真实恢复（applyPinnedTenetsRecovery）必须接收明确批准的源文件与逐条决策，
 * 并复用与迁移相同的批量写入（importLegacyPinnedItems）与收据机制。
 * 本模块不挂到任何启动路径；CLI 见 scripts/pinned-tenets-recovery.mjs。
 */

import fs from "fs";
import path from "path";
import { createHash } from "node:crypto";
import {
  PINNED_MD,
  PINNED_JSON,
  PINNED_TENETS_MIGRATION_RECEIPT,
  readLegacyPinnedSource,
  readPinnedTenetsMigrationReceipt,
  sha256File,
  type MigrationReceipt,
} from "./pinned-tenets-migration.ts";
import {
  dedupKey,
  importLegacyPinnedItems,
  legacyContentKey,
  listTenets,
  type LegacyPinImportEntry,
  type LegacyPinImportItem,
} from "../lib/memory/tenets.ts";
import { atomicWriteSync } from "../shared/safe-fs.ts";

const ARCHIVED_SOURCE_RE = /^(pinned-memory\.json|pinned\.md)\.migrated(-[0-9a-f]{8})?$/;

export interface RecoveryCandidate {
  source: string;
  legacyId: string | null;
  contentHash: string;
  /** 供用户本地决策的预览（本工具只在用户本机运行，预览不进入收据/日志） */
  preview: string;
  classification: "present" | "inactive" | "similar" | "missing";
  matchedTenetId?: string | null;
}

export interface RecoveryAgentReport {
  agentId: string;
  receiptState: string | null;
  archivedSources: Array<{ file: string; sha256: string; itemCount: number }>;
  candidates: RecoveryCandidate[];
  recoverableCount: number;
}

export interface RecoveryReport {
  home: string;
  agents: RecoveryAgentReport[];
}

export interface RecoveryApproval {
  agentId: string;
  source: string;
  decisions: Array<{ contentHash: string; action: "restore" | "skip" }>;
}

function previewOf(content: string): string {
  const oneLine = content.replace(/\s+/g, " ").trim();
  return oneLine.length > 120 ? `${oneLine.slice(0, 120)}…` : oneLine;
}

function contentHashOf(content: string): string {
  // 与迁移计划同一哈希口径（归一化内容的 sha256）
  return `sha256:${createHash("sha256").update(legacyContentKey(content), "utf-8").digest("hex")}`;
}

function classify(agentDir: string, source: string, item: LegacyPinImportItem): RecoveryCandidate {
  const content = legacyContentKey(item.content);
  const all = listTenets(agentDir);
  const exact = all.find((t) => legacyContentKey(t.content) === content);
  let classification: RecoveryCandidate["classification"] = "missing";
  let matchedTenetId: string | null = null;
  if (exact) {
    classification = exact.status === "active" ? "present" : "inactive";
    matchedTenetId = exact.id;
  } else if (all.some((t) => dedupKey(t.content) === dedupKey(content))) {
    classification = "similar";
  }
  return {
    source,
    legacyId: item.legacyId ?? null,
    contentHash: contentHashOf(content),
    preview: previewOf(content),
    classification,
    matchedTenetId,
  };
}

/** dry-run：只读扫描，绝不改任何文件。 */
export function scanPinnedTenetsRecovery(lingxiHome: string): RecoveryReport {
  const report: RecoveryReport = { home: lingxiHome, agents: [] };
  const agentsDir = path.join(lingxiHome, "agents");
  if (!fs.existsSync(agentsDir)) return report;

  for (const entry of fs.readdirSync(agentsDir, { withFileTypes: true })) {
    if (!entry.isDirectory() || entry.name.startsWith(".")) continue;
    const agentDir = path.join(agentsDir, entry.name);
    const receipt = readPinnedTenetsMigrationReceipt(agentDir);
    if (receipt?.state === "completed") continue;

    const archived = fs.readdirSync(agentDir)
      .filter((name) => ARCHIVED_SOURCE_RE.test(name))
      .sort();
    if (archived.length === 0) continue;

    const agent: RecoveryAgentReport = {
      agentId: entry.name,
      receiptState: receipt?.state ?? null,
      archivedSources: [],
      candidates: [],
      recoverableCount: 0,
    };
    for (const name of archived) {
      const filePath = path.join(agentDir, name);
      let items: LegacyPinImportItem[] = [];
      let readable = true;
      try {
        items = readLegacyPinnedSource(filePath);
      } catch {
        readable = false;
      }
      agent.archivedSources.push({
        file: name,
        sha256: sha256File(filePath),
        itemCount: readable ? items.length : -1,
      });
      for (const item of items) {
        agent.candidates.push(classify(agentDir, name, item));
      }
    }
    agent.recoverableCount = agent.candidates.filter(
      (c) => c.classification === "missing" || c.classification === "inactive",
    ).length;
    report.agents.push(agent);
  }
  return report;
}

/**
 * 真实恢复：只恢复批准清单中 action=restore 的条目；不默认全量复活；
 * 归档源文件保留不删。幂等（已 active 的精确重复不会产生新条目）。
 */
export function applyPinnedTenetsRecovery(
  lingxiHome: string,
  approval: RecoveryApproval,
): { restored: number; entries: LegacyPinImportEntry[] } {
  if (!approval || typeof approval.agentId !== "string" || !approval.agentId) {
    throw new Error("approval.agentId is required");
  }
  if (approval.agentId.includes("/") || approval.agentId.includes("\\") || approval.agentId.startsWith(".")) {
    throw new Error(`invalid agent id: ${approval.agentId}`);
  }
  const agentDir = path.join(lingxiHome, "agents", approval.agentId);
  if (!fs.existsSync(agentDir)) {
    throw new Error(`agent not found: ${approval.agentId}`);
  }
  if (typeof approval.source !== "string" || !ARCHIVED_SOURCE_RE.test(approval.source)) {
    throw new Error(`invalid recovery source: ${approval.source} (must be an archived pinned source file name)`);
  }
  const sourcePath = path.join(agentDir, approval.source);
  if (!fs.existsSync(sourcePath)) {
    throw new Error(`recovery source not found: ${approval.source}`);
  }
  if (!Array.isArray(approval.decisions) || approval.decisions.length === 0) {
    throw new Error("approval.decisions must be a non-empty array");
  }

  const items = readLegacyPinnedSource(sourcePath);
  const byHash = new Map(items.map((item) => [contentHashOf(legacyContentKey(item.content)), item]));
  const restore: LegacyPinImportItem[] = [];
  for (const decision of approval.decisions) {
    if (!decision || (decision.action !== "restore" && decision.action !== "skip")) {
      throw new Error(`invalid decision action: ${(decision as any)?.action}`);
    }
    const item = byHash.get(decision.contentHash);
    if (!item) {
      throw new Error(`decision contentHash does not match any item in ${approval.source}: ${decision.contentHash}`);
    }
    if (decision.action === "restore") restore.push(item);
  }
  if (restore.length === 0) {
    throw new Error("no restore decisions supplied; refusing to apply an empty recovery");
  }

  const { entries } = importLegacyPinnedItems(agentDir, restore);

  const receipt: MigrationReceipt = {
    version: 2,
    kind: "recovery",
    agentId: approval.agentId,
    state: "completed",
    sources: [{ file: approval.source, sha256: sha256File(sourcePath), mtimeMs: fs.statSync(sourcePath).mtimeMs }],
    authority: { file: approval.source, reason: "explicit_recovery_approval" },
    target: { existed: true, sha256: null },
    resultSha256: null,
    plan: entries.map(({ normalizedContent: _c, ...entry }) => ({ ...entry, exemption: "legacy_migration" })),
    counts: {
      sourceItems: restore.length,
      added: entries.filter((e) => e.outcome === "added").length,
      duplicateActive: entries.filter((e) => e.outcome === "duplicate_active").length,
      addedOverHistory: entries.filter((e) => e.outcome === "added_over_pending_history" || e.outcome === "added_over_rejected_history").length,
      duplicateInBatch: entries.filter((e) => e.outcome === "duplicate_in_batch").length,
    },
    archived: [],
    backupDir: null,
    error: null,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
    completedAt: new Date().toISOString(),
  };
  const receiptPath = path.join(agentDir, "memory", PINNED_TENETS_MIGRATION_RECEIPT);
  fs.mkdirSync(path.dirname(receiptPath), { recursive: true });
  atomicWriteSync(receiptPath, JSON.stringify(receipt, null, 2) + "\n");

  const restored = entries.filter((e) => e.outcome !== "duplicate_active" && e.outcome !== "duplicate_in_batch").length;
  return { restored, entries };
}
