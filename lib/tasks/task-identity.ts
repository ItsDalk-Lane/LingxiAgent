/**
 * 业务 Task 身份生成器（P02-T01：taskId 统一铸造厂）。
 *
 * 背景（P00 OWNERSHIP_MAP 🔴 / P01 交接 §4）：taskId 此前有 5 种调用方自铸格式
 * （subagent-… / workflow-… / rewind-… / speech-… / 裸 ts36），TaskRegistry 无法
 * 从 ID 本身区分新旧铸造来源。本模块是 `task_` 前缀的唯一铸造厂；调用方不得
 * 再自行拼 taskId 字符串。
 *
 * 语义（与 model-call-identity 同构）：
 *   - taskId 是**业务任务**身份：同一业务任务可以被多次执行（合法重注册，
 *     见 TaskRegistry 的 attempt 栅栏），taskId 本身不复用为"单次执行"身份。
 *   - 单次执行身份 = taskId + attempt（TaskRegistry 内维护），或 runId/mc_/ma_
 *     等各自领域的执行身份；不要把 taskId 当 callId/traceId 用。
 *   - 不携带秘密或内容片段；kind 段只是可读性标注（小写 [a-z0-9]），不是类型
 *     校验来源——类型校验走 TaskRegistry 的 type 字段。
 *
 * 兼容规则：历史持久化记录中的旧格式 taskId（subagent-… 等）不迁移、不重铸；
 * TaskRegistry 仍按非空字符串接纳它们（assertText），品牌只约束新铸造路径。
 *
 * 纪律：进程内单调计数器防碰撞 + 跨进程随机段；测试可注入确定性 now/random。
 */

import type { TaskId } from "../../shared/identity-brands.ts";
import { requireTaskId } from "../../shared/identity-brands.ts";

export interface TaskIdentityFactory {
  mint(kind?: string): TaskId;
}

const TASK_PREFIX = "task";

function normalizeKind(kind: unknown): string {
  const raw = typeof kind === "string" ? kind.trim().toLowerCase() : "";
  const slug = raw.replace(/[^a-z0-9]+/g, "");
  return slug || "gen";
}

export function createTaskIdentityFactory({
  now = () => Date.now(),
  random = defaultRandom,
}: {
  now?: () => number;
  random?: () => string;
} = {}): TaskIdentityFactory {
  let sequence = 0;
  return {
    mint: (kind?: string) => {
      sequence += 1;
      return requireTaskId(
        `${TASK_PREFIX}_${normalizeKind(kind)}_${now().toString(36)}_${sequence.toString(36)}_${random()}`,
      );
    },
  };
}

function defaultRandom() {
  return Math.random().toString(36).slice(2, 8);
}

/** 进程级默认工厂。测试请用 createTaskIdentityFactory 注入确定性源。 */
const defaultFactory = createTaskIdentityFactory();

export function mintTaskId(kind?: string): TaskId {
  return defaultFactory.mint(kind);
}
