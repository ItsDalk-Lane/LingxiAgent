import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { validateAgainstSchema } from './validator.mjs';
const generated = new URL('../../../contracts/generated/', import.meta.url);
const schema = name => JSON.parse(readFileSync(new URL(`jsonschema/${name}.schema.json`, generated), 'utf8'));
const golden = name => JSON.parse(readFileSync(new URL(`golden/${name}.json`, generated), 'utf8'));
describe('桥接统一消息、附件与回执契约', () => {
  it('真实跨语言样本覆盖消息、发送、上下文、附件和三种回执', () => {
    const index = golden('index');
    for (const name of ['BridgeRole','BridgeConversationKind','BridgeAttachmentKind','BridgeAttachment','BridgeReplyContext','BridgeTargetType','BridgeReceiptStatus','BridgeWorkerCommand','BridgeWorkerEvent']) {
      expect(index.samples.some(sample => sample.type === name)).toBe(true);
    }
    for (const status of ['confirmed','failed','unknown']) {
      const receipt = golden(`bridge-receipt-${status}`);
      expect(receipt.status).toBe(status);
      validateAgainstSchema(receipt, schema('BridgeWorkerEvent'));
    }
    expect(golden('bridge-receipt-confirmed').platformMessageIds).toEqual(['actual-platform-id']);
  });
  it('平台消息及嵌套上下文不能自报身份或权限，附件不能夹带本机路径', () => {
    const message = golden('bridge-message');
    const eventSchema = schema('BridgeWorkerEvent');
    validateAgainstSchema(message, eventSchema);
    for (const key of ['role','ownerSubject','principal','permissionMode']) {
      expect(() => validateAgainstSchema({ ...message, [key]: 'owner' }, eventSchema)).toThrow();
      expect(() => validateAgainstSchema({ ...message, replyContext: { ...message.replyContext, [key]: 'owner' } }, eventSchema)).toThrow();
    }
    expect(() => validateAgainstSchema({ ...message, attachments: [{ ...message.attachments[0], path: '/private/key' }] }, eventSchema)).toThrow();
  });
  it('保留五平台回发所需的实际上下文，同时限制代次与字段名', () => {
    const send = golden('bridge-command-send');
    validateAgainstSchema(send, schema('BridgeWorkerCommand'));
    expect(send.dispatchSeq).toBe(1);
    expect(golden('bridge-message')).toMatchObject({ displayName: '平台公开昵称', avatarUrl: 'https://example.invalid/public-avatar.png' });
    expect(send.replyContext).toMatchObject({ isGroup: false, messageThreadId: 42, contextToken: '微信上下文', targetChatId: 'actual-user', targetType: 'user', guildId: 'guild', channelId: 'channel', rootId: 'root', parentId: 'parent' });
    expect(() => validateAgainstSchema({ ...send, generation: 2 ** 32 }, schema('BridgeWorkerCommand'))).toThrow();
    expect(() => validateAgainstSchema({ ...send, dispatchSeq: 0 }, schema('BridgeWorkerCommand'))).toThrow();
    expect(() => validateAgainstSchema({ ...send, dispatchSeq: 2 ** 32 }, schema('BridgeWorkerCommand'))).toThrow();
    expect(() => validateAgainstSchema({ ...send, delivery_id: send.deliveryId }, schema('BridgeWorkerCommand'))).toThrow();
  });
  it('原生效果使用独立封闭命令、能力和回执，预览不充当最终投递', () => {
    const index = golden('index');
    for (const name of ['BridgeEffectOperation','BridgeEffectStatus','BridgeWaitingReceiptMode','BridgeStreamMode','BridgeWaitingReceiptCapability','BridgeStreamCapability','BridgeEffectCapabilities','BridgeEffectRequest','BridgeEffectReceipt']) {
      expect(index.samples.some(sample => sample.type === name)).toBe(true);
    }
    for (const sample of index.samples.filter(sample => sample.type.startsWith('BridgeEffect') || sample.type === 'BridgeStreamCapability' || sample.type === 'BridgeWaitingReceiptCapability')) {
      validateAgainstSchema(golden(sample.file.replace(/\.json$/, '')), schema(sample.type));
    }
    const request = golden('bridge-effect-request');
    expect(request).toMatchObject({ inboxId: 'actual-inbox', runId: 'actual-run', sequence: 1, operation: 'stream_update' });
    for (const key of ['role','principal','permissionMode']) {
      expect(() => validateAgainstSchema({ ...request, [key]: 'owner' }, schema('BridgeEffectRequest'))).toThrow();
      expect(() => validateAgainstSchema({ ...request, replyContext: { ...request.replyContext, [key]: 'owner' } }, schema('BridgeEffectRequest'))).toThrow();
    }
    expect(() => validateAgainstSchema({ ...request, sequence: 0 }, schema('BridgeEffectRequest'))).toThrow();
    const confirmed = golden('bridge-effect-confirmed');
    expect(confirmed).toMatchObject({ operation: 'stream_finish', platformAck: true, platformMessageIds: ['actual-native-message'], deliveryId: 'real-final-outbox' });
    expect(golden('bridge-effect-unknown')).toMatchObject({ status: 'unknown', platformAck: false });
    expect(golden('bridge-effect-skipped')).toMatchObject({ status: 'skipped', platformAck: false, platformMessageIds: [] });
    const frame = golden('bridge-worker-effect-receipt');
    validateAgainstSchema(frame, schema('BridgeWorkerEvent'));
    expect(frame).toMatchObject({ kind: 'effect_receipt', receipt: confirmed });
    expect(() => validateAgainstSchema({ ...frame, receipt: { ...frame.receipt, role: 'owner' } }, schema('BridgeWorkerEvent'))).toThrow();
  });
  it('连接恢复使用正式状态帧，不能冒充首次Ready或夹带身份权限', () => {
    validateAgainstSchema(golden('bridge-connection-status'), schema('BridgeConnectionStatus'));
    for (const status of ['connecting','connected','disconnected']) {
      const frame = golden(`bridge-worker-${status}`);
      validateAgainstSchema(frame, schema('BridgeWorkerEvent'));
      expect(frame).toMatchObject({ kind: 'connection_status', status });
      if (status === 'disconnected') expect(frame.error).toBe('platform_connection_unavailable');
      else expect(frame).not.toHaveProperty('error');
      expect(() => validateAgainstSchema({ ...frame, role: 'owner' }, schema('BridgeWorkerEvent'))).toThrow();
      expect(() => validateAgainstSchema({ ...frame, status: 'ready' }, schema('BridgeWorkerEvent'))).toThrow();
    }
  });
});
