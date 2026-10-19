import { platform, sender, messageId, inboundText, record, sent } from './state.mjs';
const response = data => new Response(JSON.stringify(data), { headers: { 'content-type': 'application/json' } });
let polled = false;
export async function platformFetch(url, options = {}) {
  const address = String(url);
  if (platform === 'qq') {
    if (address.includes('getAppAccessToken')) return response({ access_token: 'PRIVATE-QQ-ACCESS-TOKEN', expires_in: 7200 });
    if (address.endsWith('/gateway')) return response({ url: 'wss://qq.platform.fixture' });
    if (address.includes('api.sgroup.qq.com') && address.endsWith('/messages')) {
      const body = JSON.parse(options.body);
      if (body.msg_id !== messageId) throw new Error('平台被动回复未携带原消息编号');
      const text = body.msg_type === 2 ? body.markdown?.content : body.content;
      if (typeof text !== 'string' || !text.trim()) throw new Error('QQ 回复缺少实际可见正文');
      return response({ id: sent(text, new URL(address).pathname) });
    }
  }
  if (platform === 'dingtalk') {
    if (address.includes('oauth2')) return response(address.includes('api.dingtalk.io') ? { accessToken: 'PRIVATE-DING-ACCESS-TOKEN', expireIn: 7200 } : { access_token: 'PRIVATE-DING-ACCESS-TOKEN', expires_in: 7200 });
    if (address.includes('gateway/connections/open')) return response({ endpoint: 'wss://dingtalk.platform.fixture', ticket: 'PRIVATE-DING-STREAM-TICKET' });
    if (address.includes('oToMessages/batchSend')) {
      const body = JSON.parse(options.body);
      if (body.userIds?.[0] !== sender) throw new Error('钉钉目标不是原发送者');
      return response({ processQueryKey: sent(body.msgParam, body.userIds[0]) });
    }
  }
  if (platform === 'wechat') {
    if (address.includes('ilink/bot/getupdates')) {
      if (!polled) {
        polled = true;
        record('inbound', { messageId });
        record('inbound_redelivery', { messageId });
        // 原始 uint64 文本经真实适配器读取，不能先在替身中转成浮点数。
        const message = `{"message_id":${messageId},"from_user_id":${JSON.stringify(sender)},"context_token":"PRIVATE-WECHAT-CONTEXT","item_list":[{"type":1,"text_item":{"text":${JSON.stringify(inboundText)}}}]}`;
        return new Response(`{"ret":0,"get_updates_buf":"cursor","msgs":[${message},${message}]}`, { headers: { 'content-type': 'application/json' } });
      }
      return new Promise((_resolve, reject) => {
        const abort = () => reject(new DOMException('测试轮询已取消', 'AbortError'));
        if (options.signal?.aborted) abort(); else options.signal?.addEventListener('abort', abort, { once: true });
      });
    }
    if (address.includes('ilink/bot/sendmessage')) {
      const body = JSON.parse(options.body);
      if (body.msg.to_user_id !== sender || body.msg.context_token !== 'PRIVATE-WECHAT-CONTEXT') throw new Error('微信发送未使用真实冻结上下文');
      return response({ ret: 0, message_id: sent(body.msg.item_list?.[0]?.text_item?.text, sender) });
    }
  }
  // 未登记请求直接失败，测试过程不能退回真实外网。
  throw new Error(`测试未登记的平台 HTTP 请求：${new URL(address).origin}${new URL(address).pathname}`);
}
