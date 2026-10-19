// 仅替换 Telegram 外部 SDK；事件交给正式 telegram-adapter 与 worker 处理。
import { EventEmitter } from 'node:events';
import { sender, name, messageId, inboundText, record, sent } from './state.mjs';
export default class TelegramBot extends EventEmitter {
  constructor(_token, options) {
    super();
    if (!options?.polling) throw new Error('测试仅支持正式长轮询入口');
    record('connect');
    this.timer = setTimeout(() => {
      const message = { message_id: Number(messageId), chat: { id: Number(sender), type: 'private' }, from: { id: Number(sender), first_name: name }, text: inboundText };
      record('inbound', { messageId });
      // 两次原生事件经过产品适配器，不能在夹具里代替宿主去重。
      this.emit('message', message); record('inbound_redelivery', { messageId }); this.emit('message', message);
    }, 160);
  }
  async getMe() { return { id: 9000, username: 'fixture_bot' }; }
  async getUserProfilePhotos() { return { photos: [] }; }
  async getFileLink() { throw new Error('此矩阵没有登记媒体资源'); }
  async sendMessage(chatId, text) {
    if (String(chatId) !== sender) throw new Error('Telegram 发送目标不是原发送者');
    return { message_id: Number(sent(text, String(chatId))) };
  }
  async stopPolling() { clearTimeout(this.timer); record('close'); }
}
