import { sender, name, messageId, inboundText, record, sent, options } from './state.mjs';
export const Domain = { Feishu: 0, Lark: 1 };
export const LoggerLevel = { warn: 'warn' };
export class EventDispatcher { register(handlers) { this.handlers = handlers; return this; } }
export class WSClient {
  wsConfig = { wsInstance: { readyState: 1 } };
  async start({ eventDispatcher }) {
    record('connect');
    this.timer = setTimeout(async () => {
      const message = { sender: { sender_type: 'user', sender_id: { open_id: sender } }, message: { message_id: messageId, chat_id: 'native-feishu-conversation', chat_type: 'p2p', message_type: 'text', content: JSON.stringify({ text: inboundText }) } };
      record('inbound', { messageId });
      await eventDispatcher.handlers['im.message.receive_v1'](message);
      record('inbound_redelivery', { messageId });
      await eventDispatcher.handlers['im.message.receive_v1'](message);
    }, 700);
  }
  async close() { clearTimeout(this.timer); this.wsConfig.wsInstance.readyState = 3; record('close'); }
}
export class Client {
  contact = { user: { get: async () => ({ code: 0, data: { user: { name } } }) } };
  im = { message: {
    create: async ({ data }) => {
      if (options.receiptUnknown && data.content.includes('正在准备回复')) {
        record('receipt_unknown', { text: data.content, target: data.receive_id });
        throw new Error('测试平台：等待提示请求已发出，结果无法确认');
      }
      return { code: 0, data: { message_id: sent(data.content, data.receive_id) } };
    },
    update: async ({ path, data }) => { record('update', { messageId: path.message_id, text: data.content }); return { code: 0 }; },
  } };
}
