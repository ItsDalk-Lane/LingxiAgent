// 故意错误直接调用生产实现；只由 strict 门禁负例注入，不进入正常工程。
import { TaskRegistry } from '../../../lib/task-registry.ts';
import { parseSessionCreateInput } from '../../../server/session-create-input.ts';
import { createToolSchemaValidator } from '../../../lib/tools/invocation/schema-validator.ts';
import { mintModelTraceId } from '../../../lib/llm/model-call-identity.ts';

const registry = new TaskRegistry();
registry.complete('task', 'result', { expectedAttempt: '2' });
export const missingAttempt = registry.query('missing').attempt;
parseSessionCreateInput({}, 'unsupported');
createToolSchemaValidator({}, { targetId: mintModelTraceId(), origin: 'first-party', sourceId: 'source', localName: 'read', publicName: 'read', capabilityBase: 'read' });
