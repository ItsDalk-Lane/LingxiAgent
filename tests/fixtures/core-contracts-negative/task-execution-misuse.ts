// 实际生产句柄/媒体批次实现的编译反例；不得进入正常工程。
import { TaskRegistry } from '../../../lib/task-registry.ts';
import { registerTaskExecution, TaskVisibilityAttempts } from '../../../lib/tasks/task-execution.ts';
const execution = registerTaskExecution(new TaskRegistry(), 'task', { type: 'test' });
execution.attempt = 2;
execution.complete('result', { expectedAttempt: 2 });
new TaskVisibilityAttempts().bind('task', 1, '2');
new TaskVisibilityAttempts().remove({ request: async () => null }, 'task', '2');
