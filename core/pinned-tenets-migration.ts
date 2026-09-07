/**
 * pinned-tenets-migration.ts — 一次性迁移：旧「置顶记忆」文件并入 tenets 库。
 *
 * 背景：置顶记忆（pinned.md + pinned-memory.json）与用户原则（tenets.json）
 * 已合并为单一「置顶与原则」库（tenets 为底座）。本迁移把每个 agent 目录下
 * 尚未迁移的 pins 数据读出（json 优先、md 兜底），作为 user_direct/active
 * 条目并入 tenets.json（重复内容按归一化去重跳过），然后把旧文件改名
 * *.migrated ——改名而非删除：防 md-mtime 回读逻辑复活旧数据，同时保留
 * 用户数据可追溯（不自动删除任何用户内容）。
 *
 * 幂等：文件已改名或不存在即跳过；并入前按内容查重。
 */

import fs from "fs";
import path from "path";
import { createModuleLogger } from "../lib/debug-log.ts";
import { addTenetDirect } from "../lib/memory/tenets.ts";

const log = createModuleLogger("pins-migration");

const PINNED_MD = "pinned.md";
const PINNED_JSON = "pinned-memory.json";

function parseMarkdownItems(content: string): string[] {
  const text = String(content ?? "").replace(/\r\n/g, "\n").replace(/\r/g, "\n");
  const lines = text.endsWith("\n") ? text.slice(0, -1).split("\n") : text.split("\n");
  const rawItems: string[] = [];
  let current: string | null = null;

  for (const line of lines) {
    const bullet = line.match(/^-\s(.*)$/);
    if (bullet) {
      if (current !== null) rawItems.push(current);
      current = bullet[1];
      continue;
    }
    if (current === null) {
      if (line.trim()) current = line;
      continue;
    }
    current += `\n${line.replace(/^ {2}/, "")}`;
  }
  if (current !== null) rawItems.push(current);
  return rawItems.map((item) => item.trim()).filter(Boolean);
}

function readPinnedItems(agentDir: string): string[] {
  // json 优先（与旧读取一致：md mtime 更新时以 md 为准——迁移场景下直接
  // 读 json，json 缺失才解析 md）。
  try {
    const raw = JSON.parse(fs.readFileSync(path.join(agentDir, PINNED_JSON), "utf-8"));
    if (Array.isArray(raw?.items)) {
      const items = raw.items
        .map((item: any) => String(item?.content ?? "").replace(/\r\n/g, "\n").trim())
        .filter(Boolean);
      if (items.length > 0 || fs.existsSync(path.join(agentDir, PINNED_MD))) {
        return items;
      }
    }
  } catch (err: any) {
    if (err?.code !== "ENOENT") {
      log.warn(`failed to read ${PINNED_JSON}, falling back to markdown: ${err?.message || err}`);
    }
  }
  try {
    return parseMarkdownItems(fs.readFileSync(path.join(agentDir, PINNED_MD), "utf-8"));
  } catch {
    return [];
  }
}

function migrateAgentDir(agentDir: string, agentId: string): void {
  const mdPath = path.join(agentDir, PINNED_MD);
  const jsonPath = path.join(agentDir, PINNED_JSON);
  const hasSource = fs.existsSync(mdPath) || fs.existsSync(jsonPath);
  if (!hasSource) return;

  const items = readPinnedItems(agentDir);
  let added = 0;
  let duplicates = 0;
  for (const content of items) {
    try {
      const result = addTenetDirect(agentDir, { content, priority: "high" });
      if (result.duplicate) duplicates += 1;
      else added += 1;
    } catch (err: any) {
      // 容量满等错误不阻塞迁移（老数据已在旧文件里，改名后仍可追溯）。
      log.warn(`[${agentId}] failed to migrate a pinned item: ${err?.message || err}`);
    }
  }

  // 改名留证：.migrated 后缀防旧读取路径复活，内容保留给用户自查。
  for (const filePath of [jsonPath, mdPath]) {
    if (fs.existsSync(filePath)) {
      try {
        fs.renameSync(filePath, `${filePath}.migrated`);
      } catch (err: any) {
        log.warn(`[${agentId}] failed to rename ${path.basename(filePath)}: ${err?.message || err}`);
      }
    }
  }

  if (added > 0 || duplicates > 0 || items.length > 0) {
    log.log(`[${agentId}] pinned→tenets migration: ${added} added, ${duplicates} duplicates, ${items.length} source items`);
  }
}

/** 启动时调用：扫全部 agent 目录执行一次性迁移。失败只记日志，不阻塞启动。 */
export function migratePinnedMemoryToTenets(lingxiHome: string): void {
  try {
    const agentsDir = path.join(lingxiHome, "agents");
    if (!fs.existsSync(agentsDir)) return;
    for (const entry of fs.readdirSync(agentsDir, { withFileTypes: true })) {
      if (!entry.isDirectory() || entry.name.startsWith(".")) continue;
      try {
        migrateAgentDir(path.join(agentsDir, entry.name), entry.name);
      } catch (err: any) {
        log.warn(`[${entry.name}] migration failed: ${err?.message || err}`);
      }
    }
  } catch (err: any) {
    log.warn(`pinned→tenets migration scan failed: ${err?.message || err}`);
  }
}
