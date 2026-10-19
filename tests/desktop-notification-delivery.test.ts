import { EventEmitter } from 'node:events';
import { afterEach, describe, expect, it, vi } from 'vitest';
import delivery from '../desktop/src/shared/desktop-notification-delivery.cjs';
const { showNotificationWithReceipt } = delivery;
function notification(action?: (item: EventEmitter) => void) {
  const item = new EventEmitter() as EventEmitter & { show: ReturnType<typeof vi.fn> };
  item.show = vi.fn(() => action?.(item));
  return item;
}
afterEach(() => vi.useRealTimers());
describe('系统通知真实展示回执', () => {
  it('提交后等待系统展示事件，完成后回收临时监听', async () => {
    const item=notification();const completed=vi.fn();
    const pending=showNotificationWithReceipt(item).then((value: unknown)=>{completed();return value;});
    await Promise.resolve();expect(item.show).toHaveBeenCalledOnce();expect(completed).not.toHaveBeenCalled();
    item.emit('show');expect(await pending).toEqual({shown:true});
    expect(item.listenerCount('show')+item.listenerCount('failed')+item.listenerCount('close')).toBe(0);
  });
  it('同步展示事件也能收到，失败事件不能冒充送达', async () => {
    expect(await showNotificationWithReceipt(notification(item=>item.emit('show')))).toEqual({shown:true});
    expect(await showNotificationWithReceipt(notification(item=>item.emit('failed',{},'denied')))).toEqual({shown:false,reason:'failed'});
  });
  it('没有回执时明确未知，迟到事件不会重复完成', async () => {
    vi.useFakeTimers();const item=notification();const done=vi.fn();
    const pending=showNotificationWithReceipt(item,{timeoutMs:10}).then((value:unknown)=>{done();return value;});
    await vi.advanceTimersByTimeAsync(10);expect(await pending).toEqual({shown:false,reason:'unconfirmed'});
    item.emit('show');expect(done).toHaveBeenCalledOnce();expect(item.listenerCount('show')).toBe(0);
  });
  it('关闭或系统调用抛错时保留未知结果', async () => {
    expect(await showNotificationWithReceipt(notification(item=>item.emit('close')))).toEqual({shown:false,reason:'unconfirmed'});
    expect(await showNotificationWithReceipt(notification(()=>{throw new Error('native boundary');}))).toEqual({shown:false,reason:'unconfirmed'});
  });
});
