import { EventEmitter } from 'node:events';
import { platform, sender, name, messageId, inboundText, record, options } from './state.mjs';
let connectionCount = 0;
export default class PlatformWebSocket extends EventEmitter {
  static OPEN = 1; static CLOSED = 3;
  readyState = 1;
  constructor(url) {
    super();
    if (!['wss://qq.platform.fixture', 'wss://dingtalk.platform.fixture'].some(expected => String(url).startsWith(expected))) throw new Error('测试只允许登记的平台 WebSocket');
    record('connect');
    const connection = ++connectionCount;
    this.openTimer = setTimeout(() => {
      this.emit('open');
      if (platform === 'qq') this.emit('message', JSON.stringify({ op: 0, t: 'READY', d: { session_id: 'native-gateway-session' } }));
    }, 20);
    if (options.transientDrop && connection === 1) {
      this.dropTimer = setTimeout(() => {
        record('network_disconnected');
        this.emit('error', new Error('测试平台：短暂断网'));
        this.close();
      }, 80);
    }
    this.messageTimer = setTimeout(() => {
      record('inbound', { messageId });
      const payload = platform === 'qq'
        ? { op: 0, t: 'C2C_MESSAGE_CREATE', d: { id: messageId, author: { id: sender, user_openid: sender, username: name }, content: inboundText } }
        : { type: 'CALLBACK', headers: { topic: '/v1.0/im/bot/messages/get', messageId: 'native-transport' }, data: JSON.stringify({ conversationId: 'native-ding-conversation', conversationType: '1', msgId: messageId, senderStaffId: sender, senderNick: name, msgtype: 'text', text: { content: inboundText } }) };
      // 同一平台编号重投；去重由正式宿主和数据库负责。
      this.emit('message', JSON.stringify(payload)); record('inbound_redelivery', { messageId }); this.emit('message', JSON.stringify(payload));
    }, 160);
  }
  send() {}
  close() { if (this.readyState === 3) return; clearTimeout(this.openTimer); clearTimeout(this.messageTimer); clearTimeout(this.dropTimer); this.readyState = 3; record('close'); this.emit('close', 1000); }
  terminate() { this.close(); }
}
