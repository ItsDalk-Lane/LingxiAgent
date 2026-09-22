import type { TaskRegistry, TaskRegistration } from '../task-registry.ts';

/** 既有注册表的窄接口；不持有第二份任务状态。 */
export type TaskRegistryClient = Partial<Pick<TaskRegistry,
  'register' | 'registerHandler' | 'query' | 'complete' | 'fail' | 'remove'>>;

export interface TaskExecution {
  readonly taskId: string;
  readonly attempt: number | null;
  complete(result: unknown): ReturnType<TaskRegistry['complete']> | undefined;
  fail(error: unknown): ReturnType<TaskRegistry['fail']> | undefined;
  remove(): ReturnType<TaskRegistry['remove']> | undefined;
}

function positiveAttempt(value: unknown): number | null {
  return typeof value === 'number' && Number.isSafeInteger(value) && value > 0 ? value : null;
}

/** 登记时捕获宿主返回的批次；回调不能查询最新批次或另传批次冒认。 */
export function registerTaskExecution(
  registry: TaskRegistryClient | null | undefined,
  taskId: string,
  input: TaskRegistration,
): TaskExecution {
  const task = registry?.register?.(taskId, input);
  const attempt = positiveAttempt(task?.attempt);
  const options = Object.freeze({ expectedAttempt: attempt });
  return Object.freeze({
    taskId,
    attempt,
    complete: (result: unknown) => registry?.complete?.(taskId, result, options),
    fail: (error: unknown) => registry?.fail?.(taskId, error, options),
    remove: () => registry?.remove?.(taskId, options),
  });
}

export interface TaskRemovalBus {
  request(topic: 'task:remove', payload: { taskId: string; expectedAttempt: number | null }): Promise<unknown>;
}

/** 现有媒体轮询器的可见性批次索引；键按媒体批次隔离，不与注册表批次混用。 */
export class TaskVisibilityAttempts {
  private readonly attempts = new Map<string, number>();

  bind(taskId: string, mediaAttempt: number, registryAttempt: number | undefined): void {
    const attempt = positiveAttempt(registryAttempt);
    if (positiveAttempt(mediaAttempt) === null) return;
    if (attempt !== null) this.attempts.set(`${taskId}:${mediaAttempt}`, attempt);
  }

  bindReceipt(taskId: string, mediaAttempt: number, receipt: unknown): void {
    if (receipt === null || typeof receipt !== 'object' || !('task' in receipt)) return;
    const task = receipt.task;
    if (task === null || typeof task !== 'object' || !('attempt' in task) || !('taskId' in task) || task.taskId !== taskId) return;
    const attempt = positiveAttempt(task.attempt);
    if (attempt !== null) this.bind(taskId, mediaAttempt, attempt);
  }

  async remove(bus: TaskRemovalBus, taskId: string, mediaAttempt: number): Promise<unknown> {
    const key = `${taskId}:${mediaAttempt}`;
    const expectedAttempt = this.attempts.get(key) ?? null;
    try {
      return await bus.request('task:remove', { taskId, expectedAttempt });
    } finally {
      // 权威结果此前已交接；诊断清理失败也不能永久保留这次绑定。错误继续向上传递。
      this.attempts.delete(key);
    }
  }
}
