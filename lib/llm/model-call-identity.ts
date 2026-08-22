/**
 * Model Call 身份生成器。
 *
 * 三层身份（语义严格区分，不得混用）：
 *   - callId    `mc_…`  一次 logical model call（业务意义上的一次模型调用），
 *                       在 Provider 请求发生之前生成，success/error/abort 共用。
 *   - attemptId `ma_…`  一次真实/逻辑网络 attempt；Provider/SDK retry 属于同一
 *                       callId 下的不同 attemptId。
 *   - traceId   `mt_…`  一个用户任务触发的多次模型调用之间的关联根（本轮仅
 *                       在 schema/scope 中透传，不强制生成）。
 *
 * 纪律：
 *   - 不依赖数据库自增；进程内靠单调计数器防碰撞，跨进程靠随机段。
 *   - 不携带任何秘密或内容片段，适合直接进日志。
 *   - 测试可注入确定性 now/random，得到可复现 ID。
 */

export interface ModelCallIdentityFactory {
  mintCallId(): string;
  mintAttemptId(): string;
  mintTraceId(): string;
}

const CALL_PREFIX = "mc";
const ATTEMPT_PREFIX = "ma";
const TRACE_PREFIX = "mt";

export function createModelCallIdentityFactory({
  now = () => Date.now(),
  random = defaultRandom,
}: {
  now?: () => number;
  random?: () => string;
} = {}): ModelCallIdentityFactory {
  let sequence = 0;
  const mint = (prefix: string) => {
    sequence += 1;
    return `${prefix}_${now().toString(36)}_${sequence.toString(36)}_${random()}`;
  };
  return {
    mintCallId: () => mint(CALL_PREFIX),
    mintAttemptId: () => mint(ATTEMPT_PREFIX),
    mintTraceId: () => mint(TRACE_PREFIX),
  };
}

function defaultRandom() {
  return Math.random().toString(36).slice(2, 8);
}

/** 进程级默认工厂。测试请用 createModelCallIdentityFactory 注入确定性源。 */
const defaultFactory = createModelCallIdentityFactory();

export function mintModelCallId(): string {
  return defaultFactory.mintCallId();
}

export function mintModelAttemptId(): string {
  return defaultFactory.mintAttemptId();
}

export function mintModelTraceId(): string {
  return defaultFactory.mintTraceId();
}
