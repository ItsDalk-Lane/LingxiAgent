/**
 * history-run-merge.ts — 分页补页与既有会话内容的幂等合并（F1/F2 修复链）
 *
 * 职责：
 *  - 同 Run 的更早页片段（runKey 相同）缝进既有 Run 项并重投影：一个逻辑 Run
 *    始终一个显示项、一个终态，显示项 id 保持 Run 尾部记录 id（React key 稳定）；
 *  - 同一事实重复到达幂等：按原始记录 displayId / 显示项 id 去重，不增加消息、
 *    工具或告警数量；新版本按身份受控更新（历史记录不可变，同 id 即同事实）；
 *  - 不同 Run、不同会话、interlude 不混入：缝合仅认 runKey + 显示 id 边界。
 *
 * 坐标纪律：runKey/displayId 是服务端 display 序号坐标系（原始记录身份），
 * 与 UI 显示项 id、React key、分页边界各自独立，绝不复用（不变量四）。
 */

import type { ChatListItem, ChatMessage, HistoryRunFacts, HistoryRunRecordFact } from '../stores/chat-types';
import { projectHistoryRunFromFacts } from '../utils/history-builder';

function itemIdentity(item: ChatListItem): string | null {
  if (item.type === 'message') return item.data.id;
  return item.id;
}

function isAssistantMessageItem(item: ChatListItem): boolean {
  return item.type === 'message' && item.data.role === 'assistant';
}

function mergeRunFacts(
  existing: HistoryRunFacts,
  fragment: HistoryRunFacts,
): HistoryRunFacts {
  const byDisplayId = new Map<string, HistoryRunRecordFact>();
  for (const record of existing.records) byDisplayId.set(record.displayId, record);
  for (const record of fragment.records) {
    // 同 displayId 重复到达：保留首见（历史记录不可变）；不同 runKey 的记录
    // 不会到这里（调用方已按 runKey 过滤）。
    if (!byDisplayId.has(record.displayId)) byDisplayId.set(record.displayId, record);
  }
  const records = [...byDisplayId.values()].sort((a, b) => Number(a.displayId) - Number(b.displayId));
  // 组内首条记录变了 → Run 级元数据（输入绑定/时间戳）跟随新的头部记录，
  // 与全量恢复（all=1）时的取值一致。
  return {
    ...existing,
    ...(fragment.turnInputEntryId !== undefined ? { turnInputEntryId: fragment.turnInputEntryId } : {}),
    ...(fragment.turnInputVisible !== undefined ? { turnInputVisible: fragment.turnInputVisible } : {}),
    ...(fragment.firstRecordTimestamp !== undefined ? { firstRecordTimestamp: fragment.firstRecordTimestamp } : {}),
    records,
  };
}

export interface MergePrependedItemsResult {
  items: ChatListItem[];
  /** 是否发生了 Run 片段缝合（诊断/性能记录用）。 */
  stitchedRunKeys: string[];
}

/**
 * 把一页更早的历史项合并进既有会话项列表。
 *
 * 前提：incoming 整体早于 existing（分页只向更早方向推进）。
 * 处理顺序：先按显示身份去重 incoming，再识别 incoming 末尾的 Run 片段与
 * existing 首个同 runKey 的 Run 项缝合重投影。
 */
export function mergePrependedHistoryItems(
  existing: ChatListItem[],
  incoming: ChatListItem[],
): MergePrependedItemsResult {
  const stitchedRunKeys: string[] = [];
  if (incoming.length === 0) return { items: existing, stitchedRunKeys };
  if (existing.length === 0) return { items: incoming, stitchedRunKeys };

  // 1) 识别缝合对：incoming 中最后一个带 runFacts 的助手片段 ↔ existing 中
  //    第一个同 runKey 的助手项（existing 的该项同样以 runFacts 标注可缝）。
  let fragmentIdx = -1;
  for (let i = incoming.length - 1; i >= 0; i -= 1) {
    if (isAssistantMessageItem(incoming[i]) && (incoming[i] as { data: ChatMessage }).data.runFacts) {
      fragmentIdx = i;
      break;
    }
  }
  const fragment = fragmentIdx >= 0
    ? (incoming[fragmentIdx] as { type: 'message'; data: ChatMessage }).data
    : null;
  const fragmentFacts = fragment?.runFacts ?? null;

  let stitchTargetIdx = -1;
  if (fragmentFacts) {
    for (let i = 0; i < existing.length; i += 1) {
      if (!isAssistantMessageItem(existing[i])) continue;
      const targetFacts = (existing[i] as { data: ChatMessage }).data.runFacts;
      if (targetFacts && targetFacts.runKey === fragmentFacts.runKey) {
        stitchTargetIdx = i;
        break;
      }
    }
  }

  // 2) incoming 去重：同显示身份已存在于 existing（或本页缝合对内部）的项丢弃。
  const existingIds = new Set<string>();
  for (const item of existing) {
    const id = itemIdentity(item);
    if (id != null) existingIds.add(id);
  }
  const keptIncoming: ChatListItem[] = [];
  const seenIncomingIds = new Set<string>();
  for (let i = 0; i < incoming.length; i += 1) {
    if (fragmentIdx >= 0 && i === fragmentIdx) continue; // 缝合对单独处理
    const id = itemIdentity(incoming[i]);
    if (id != null) {
      if (existingIds.has(id) || seenIncomingIds.has(id)) continue;
      seenIncomingIds.add(id);
    }
    keptIncoming.push(incoming[i]);
  }

  if (stitchTargetIdx < 0 || !fragment || !fragmentFacts) {
    return { items: [...keptIncoming, ...existing], stitchedRunKeys };
  }

  // 3) 缝合：合并 facts（原始记录并集）→ 重投影 → 替换既有项内容（位置与 id 不变）。
  const targetItem = existing[stitchTargetIdx];
  if (targetItem.type !== 'message') {
    return { items: [...keptIncoming, ...existing], stitchedRunKeys };
  }
  const targetFacts = targetItem.data.runFacts!;
  const mergedFacts = mergeRunFacts(targetFacts, fragmentFacts);
  const mergedMessage = projectHistoryRunFromFacts(mergedFacts);
  // Run 头部已加载（最小 displayId == turnStartIndex）→ 不再携带缝合事实（内存有界）。
  const minDisplayId = mergedFacts.records.length
    ? Number(mergedFacts.records[0].displayId)
    : Number.NaN;
  const runComplete = minDisplayId === mergedFacts.turnStartIndex;
  if (!runComplete) mergedMessage.runFacts = mergedFacts;
  const nextExisting = [...existing];
  nextExisting[stitchTargetIdx] = { type: 'message', data: mergedMessage };
  stitchedRunKeys.push(mergedFacts.runKey);
  return { items: [...keptIncoming, ...nextExisting], stitchedRunKeys };
}
