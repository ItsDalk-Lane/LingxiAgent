/**
 * 会话 JSONL 变更世代（阶段 C / C02 可信追加依据）
 *
 * 进程内内存计数器：每个已规范路径一个单调递增 epoch。仅作历史目录缓存的失效
 * 提示——不是新的持久化事实源，不改变写入次序、持久化形状或 SDK 行为。
 *
 * 规则：
 *  - 追加路径（SDK appendFileSync 正常追加）**不**递增——这是可信追加的前提；
 *  - 一切可能改写已读前缀的写入（整文件重写 / 修复 / 快照 flush / 迁移 / 删除 /
 *    归档 rename）必须在「旧偏移可能失效之前」递增（写前调用）；
 *  - epoch 单调不回退：写失败不得把旧目录标回可信。
 *
 * 信任边界：本机制只覆盖单进程内已知写入模型 + 下列已插桩路径；任意外部进程
 * in-place 改写未读前缀不在承诺范围（无统一权威通知，此类变化与纯追加不可区分，
 * 属已声明边界；替换/删除仍由 dev/ino/ctime 校验捕获）。
 */
import path from "path";

export type SessionFileMutationKind =
  | "rewrite"          // 整文件重写（writeSessionEntriesFile / _rewriteFile / flush snapshot）
  | "repair"           // 修复重写（超大行 / 孤儿 toolResult / inline media）
  | "migration"        // 打开时 v1/v2 → v3 迁移重写
  | "rename"           // 归档/恢复/移动（源与目标都需递增）
  | "delete"           // 删除
  | "locator_rebind";  // locator 重新绑定

interface MutationEntry {
  epoch: number;
  lastKind: SessionFileMutationKind | null;
}

const registry = new Map<string, MutationEntry>();

function normalize(filePath: string): string {
  return path.resolve(filePath);
}

/** 写前调用：递增该路径的变更世代。 */
export function noteSessionFileMutation(filePath: string, kind: SessionFileMutationKind): void {
  const key = normalize(filePath);
  const entry = registry.get(key);
  if (entry) {
    entry.epoch += 1;
    entry.lastKind = kind;
  } else {
    registry.set(key, { epoch: 1, lastKind: kind });
  }
}

/** 当前世代（无记录 = 0）。 */
export function sessionFileMutationEpoch(filePath: string): number {
  return registry.get(normalize(filePath))?.epoch ?? 0;
}

/** 诊断用：最近变更类型。 */
export function sessionFileMutationLastKind(filePath: string): SessionFileMutationKind | null {
  return registry.get(normalize(filePath))?.lastKind ?? null;
}

/** 测试用：清空全部世代（不用于生产路径）。 */
export function resetSessionFileMutationEpochsForTest(): void {
  registry.clear();
}
