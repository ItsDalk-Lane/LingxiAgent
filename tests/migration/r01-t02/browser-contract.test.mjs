import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { validateAgainstSchema } from './validator.mjs';

const generated = new URL('../../../contracts/generated/', import.meta.url);
const schema = name => JSON.parse(readFileSync(new URL(`jsonschema/${name}.schema.json`, generated), 'utf8'));
const golden = name => JSON.parse(readFileSync(new URL(`golden/${name}.json`, generated), 'utf8'));

describe('正式浏览器的跨语言接口边界', () => {
  it('七种统一类型都有正式生成样本', () => {
    const registered = new Set(golden('index').samples.map(sample => sample.type));
    for (const name of ['BrowserAction', 'BrowserCommand', 'BrowserOpenBehavior', 'BrowserSettings', 'BrowserTab', 'BrowserWorkspace', 'BrowserReply']) {
      expect(registered.has(name)).toBe(true);
    }
  });
  it('客户端命令不能携带服务授予的身份与授权', () => {
    const command = { action: 'navigate', url: 'https://example.org/' };
    expect(() => validateAgainstSchema(command, schema('BrowserCommand'))).not.toThrow();
    for (const field of ['sessionId', 'ownerSubject', 'executionNode', 'epoch', 'human', 'runId', 'permission']) {
      expect(() => validateAgainstSchema({ ...command, [field]: 'forged' }, schema('BrowserCommand'))).toThrow();
    }
  });
  it('浏览器设置的封闭补丁与服务端执行节点保持不同含义', () => {
    expect(() => validateAgainstSchema({ acceptCookies: false, agentOpenBehavior: 'current_tab' }, schema('BrowserSettings'))).not.toThrow();
    expect(() => validateAgainstSchema({ acceptCookies: false, executionNode: 'client' }, schema('BrowserSettings'))).toThrow();
    const workspace = golden('browser-workspace');
    expect(typeof workspace.executionNode).toBe('string');
    expect(workspace.sessionPath).toBe(`native:${workspace.sessionId}`);
    expect(() => validateAgainstSchema(workspace, schema('BrowserWorkspace'))).not.toThrow();
  });
  it('默认数组注释不修改输入，也不能放过错误类型或未知关键字', () => {
    const reply = golden('browser-reply');
    expect(() => validateAgainstSchema(reply, schema('BrowserReply'))).not.toThrow();
    expect(() => validateAgainstSchema({ ...reply, files: 'forged' }, schema('BrowserReply'))).toThrow();
    const input = {};
    expect(() => validateAgainstSchema(input, { type: 'object', properties: { receipts: { type: 'array', items: { type: 'string' }, default: [] } } })).not.toThrow();
    expect(input).toEqual({});
    expect(() => validateAgainstSchema([1], { type: 'array', items: { type: 'string' }, default: [] })).toThrow();
    expect(() => validateAgainstSchema([], { type: 'array', unknownConstraint: true })).toThrow();
  });

});
