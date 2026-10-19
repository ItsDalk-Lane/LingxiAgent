import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { validateAgainstSchema } from './validator.mjs';

const generated = new URL('../../../contracts/generated/', import.meta.url);
const schema = name => JSON.parse(readFileSync(new URL(`jsonschema/${name}.schema.json`, generated), 'utf8'));
const golden = name => JSON.parse(readFileSync(new URL(`golden/${name}.json`, generated), 'utf8'));

describe('后台通知的 Rust 生成契约', () => {
  it('五种统一类型都有真实跨语言样本', () => {
    const index = golden('index');
    const registered = new Set(index.samples.map(sample => sample.type));
    for (const name of ['AutomationNotificationStatus', 'AutomationNotificationReceipt', 'AutomationNotificationClaim', 'AutomationDisplayReceipt', 'AutomationNotificationCommand']) {
      expect(registered.has(name)).toBe(true);
    }
    for (const status of ['queued', 'started', 'confirmed', 'unknown', 'failed']) {
      const receipt = golden(`automation-notification-${status}`);
      expect(receipt.status).toBe(status);
      expect(() => validateAgainstSchema(receipt, schema('AutomationNotificationReceipt'))).not.toThrow();
    }
  });

  it('领取样本只有开始凭据，真实显示凭据属于独立确认命令', () => {
    const claim = golden('automation-notification-claim');
    expect(claim.reserved).toBe(true);
    expect(claim.receipt.status).toBe('started');
    expect(claim.receipt.platformReceipt).toBeNull();
    const ack = golden('automation-command-ack');
    expect(ack).toMatchObject({ action: 'ack', platformReceipt: { deliveryId: ack.deliveryId, channel: 'desktop', displayed: true } });
  });

  it('生成的命令和嵌套回执拒绝额外字段、错名和未知动作', () => {
    const commandSchema = schema('AutomationNotificationCommand');
    for (const fixture of ['claim', 'ack', 'failed', 'unknown']) {
      expect(() => validateAgainstSchema(golden(`automation-command-${fixture}`), commandSchema)).not.toThrow();
    }
    for (const invalid of [
      { action: 'claim', deliveryId: 'd1', displayed: true },
      { action: 'ack', deliveryId: 'd1', platformReceipt: { deliveryId: 'd1', channel: 'desktop', displayed: true, trusted: true } },
      { action: 'claim', delivery_id: 'd1' },
      { action: 'confirm', deliveryId: 'd1' },
    ]) {
      expect(() => validateAgainstSchema(invalid, commandSchema)).toThrow();
    }
    const receipt = golden('automation-notification-started');
    expect(() => validateAgainstSchema({ ...receipt, displayed: true }, schema('AutomationNotificationReceipt'))).toThrow();
  });
});
