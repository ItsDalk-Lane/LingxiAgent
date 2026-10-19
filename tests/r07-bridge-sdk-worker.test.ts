import { EventEmitter } from "node:events";
import { PassThrough, Readable, Writable } from "node:stream";
import { spawn } from "node:child_process";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// 只替换平台 SDK 与 HTTP 响应；适配器工厂、消息转换和管道均使用产品实现。
const platform = vi.hoisted(() => ({
  telegram: [] as any[], sockets: [] as any[], feishu: {} as any,
  fetch: vi.fn(), sendTelegram: vi.fn(), sendFeishu: vi.fn(), closeFeishu: vi.fn(),
  downloadFeishu: vi.fn(), typingTelegram: vi.fn(), requestTelegram: vi.fn(),
  updateFeishu: vi.fn(), createFeishuCard: vi.fn(), settingsFeishuCard: vi.fn(), contentFeishuCard: vi.fn(), handlers: {} as any,
}));
vi.mock("node-telegram-bot-api", () => ({ default: class {
  handlers = new Map<string, any[]>();
  getMe = vi.fn(async () => ({ id: 1, username: "test" }));
  getFileLink = vi.fn(async () => "https://api.telegram.org/file/test/asset");
  getUserProfilePhotos = vi.fn(async () => ({ photos: [] }));
  sendMessage = platform.sendTelegram; sendPhoto = platform.sendTelegram;
  sendVideo = platform.sendTelegram; sendAudio = platform.sendTelegram; sendDocument = platform.sendTelegram;
  sendChatAction = platform.typingTelegram; _request = platform.requestTelegram;
  stopPolling = vi.fn(async () => {});
  removeAllListeners() { this.handlers.clear(); }
  on(name, callback) { const list = this.handlers.get(name) || []; list.push(callback); this.handlers.set(name, list); }
  async emit(name, data) { for (const handler of this.handlers.get(name) || []) await handler(data); }
  constructor() { platform.telegram.push(this); }
} }));
vi.mock("ws", () => ({ default: class {
  static OPEN = 1; static CLOSED = 3;
  readyState = 1; handlers = new Map<string, any[]>(); sent: any[] = [];
  closed = false;
  constructor(public url: string) { platform.sockets.push(this); }
  on(name, handler) { const list = this.handlers.get(name) || []; list.push(handler); this.handlers.set(name, list); return this; }
  emit(name, data?) { for (const handler of this.handlers.get(name) || []) handler(data); }
  send(data) { this.sent.push(JSON.parse(data)); }
  close() { this.closed = true; this.readyState = 3; this.emit("close"); }
  terminate() { this.close(); }
  removeAllListeners() { this.handlers.clear(); }
} }));
vi.mock("undici", async importOriginal => ({ ...await importOriginal<any>(), fetch: (...args) => platform.fetch(...args) }));
vi.mock("@larksuiteoapi/node-sdk", () => ({
  Domain: { Feishu: 0, Lark: 1 }, LoggerLevel: { warn: "warn" },
  EventDispatcher: class { register(handlers) { platform.handlers = handlers; return this; } },
  WSClient: class {
    wsConfig = { wsInstance: { readyState: 1 } };
    start = vi.fn(async () => {}); close = platform.closeFeishu;
  },
  Client: class {
    contact = { user: { get: async () => ({ data: { user: { name: "Alice" } } }) } };
    im = { message: { create: platform.sendFeishu, update: platform.updateFeishu }, messageResource: { get: platform.downloadFeishu },
      image: { create: async () => ({ code: 0, data: { image_key: "image-key" } }) },
      file: { create: async () => ({ code: 0, data: { file_key: "file-key" } }) } };
    cardkit = { v1: { card: { create: platform.createFeishuCard, settings: platform.settingsFeishuCard }, cardElement: { content: platform.contentFeishuCard } } };
    constructor(options) { platform.feishu = options; }
  },
}));

import { BridgeSdkWorker, createSdkAdapter, runBridgeSdkWorker, SDK_WORKER_LIMITS, sdkReceiptIds } from "../lib/bridge/sdk-worker.ts";
import { SdkEffects } from "../lib/bridge/sdk-effects.ts";
import { runSdkDiagnostic, runBridgeSdkDiagnostic } from "../lib/bridge/sdk-diagnostic.ts";

const base = { connectorId: "connector-1", generation: 7 };
const configs = {
  telegram: { agentId: "hana", token: "not-a-real-telegram-token" },
  feishu: { agentId: "hana", appId: "test-app", appSecret: "test-secret" },
  dingtalk: { agentId: "hana", authMode: "current", corpId: "corp", clientId: "test-app", clientSecret: "test-secret", robotCode: "robot" },
  qq: { agentId: "hana", appID: "test-app", appSecret: "test-secret" },
  wechat: { agentId: "hana", botToken: "not-a-real-wechat-token" },
};
const workers: BridgeSdkWorker[] = [];
const dispatchCounters = new WeakMap<BridgeSdkWorker, { next: number; recent: Map<string, number> }>();
function response(value) { return new Response(JSON.stringify(value), { headers: { "content-type": "application/json" } }); }
function dispatch(worker: BridgeSdkWorker, command: any) {
  let counter = dispatchCounters.get(worker);
  if (!counter) { counter = { next: 0, recent: new Map() }; dispatchCounters.set(worker, counter); }
  const key = JSON.stringify(command);
  let seq = counter.recent.get(key);
  if (seq === undefined) { seq = ++counter.next; counter.recent.set(key, seq); if (counter.recent.size > 512) counter.recent.delete(counter.recent.keys().next().value!); }
  return worker.accept({ dispatchSeq: seq, ...command });
}
function send(worker, overrides: any = {}) { return dispatch(worker, { ...base, kind: "send", deliveryId: "delivery-1", conversationId: "conversation-1", text: "reply", attachments: [], replyContext: { isGroup: false }, ...overrides }); }
function effect(worker: BridgeSdkWorker, operation: string, sequence: number, overrides: any = {}) {
  return dispatch(worker, { ...base, kind: "effect", effect: { effectId: `effect-${sequence}`, streamId: "stream-1", inboxId: "actual-inbox", runId: "actual-run", sequence, operation, deliveryId: operation === "stream_finish" ? "final-delivery" : null, conversationId: "conversation-1", replyContext: { isGroup: false }, text: "真实回复内容", rich: false, ...overrides } });
}
async function effectReceipt(events: any[], sequence: number) {
  await vi.waitFor(() => expect(events.some(item => item.kind === "effect_receipt" && item.receipt.sequence === sequence)).toBe(true));
  return events.find(item => item.kind === "effect_receipt" && item.receipt.sequence === sequence).receipt;
}
async function init(name: keyof typeof configs, config: any = configs[name]) {
  const events: any[] = [];
  const worker = new BridgeSdkWorker(async event => { events.push(event); }); workers.push(worker);
  await worker.accept({ ...base, kind: "init", platform: name, config });
  if (name === "qq" || name === "dingtalk") {
    await vi.waitFor(() => expect(platform.sockets.length).toBe(1));
    const socket = platform.sockets[0];
    if (name === "qq") socket.emit("message", JSON.stringify({ op: 0, t: "READY", d: { session_id: "session" } }));
    else socket.emit("open");
  }
  await vi.waitFor(() => expect(events.some(item => item.kind === "ready")).toBe(true));
  return { worker, events };
}
async function media(worker, id, bytes: Buffer) {
  await worker.accept({ ...base, kind: "media_begin", attachmentId: id, sizeBytes: bytes.length, mime: "text/plain", filename: "../display-name.txt" });
  for (let offset = 0, seq = 0; offset < bytes.length; offset += SDK_WORKER_LIMITS.chunkBytes, seq++)
    await worker.accept({ ...base, kind: "media_chunk", attachmentId: id, seq, data: bytes.subarray(offset, offset + SDK_WORKER_LIMITS.chunkBytes).toString("base64") });
  await worker.accept({ ...base, kind: "media_end", attachmentId: id });
}

beforeEach(() => {
  platform.telegram.length = 0; platform.sockets.length = 0; platform.handlers = {};
  platform.sendTelegram.mockReset().mockResolvedValue({ message_id: 9001 });
  platform.typingTelegram.mockReset().mockResolvedValue(true);
  platform.requestTelegram.mockReset().mockImplementation(async name => name === "sendRichMessage" ? { message_id: 9002 } : true);
  platform.sendFeishu.mockReset().mockResolvedValue({ code: 0, data: { message_id: "feishu-sent" } });
  platform.updateFeishu.mockReset().mockResolvedValue({ code: 0 });
  platform.createFeishuCard.mockReset().mockResolvedValue({ code: 0, data: { card_id: "actual-card" } });
  platform.settingsFeishuCard.mockReset().mockResolvedValue({ code: 0 });
  platform.contentFeishuCard.mockReset().mockResolvedValue({ code: 0 });
  platform.downloadFeishu.mockReset().mockImplementation(async () => ({ getReadableStream: () => Readable.from([Buffer.from("native-media")]) }));
  platform.closeFeishu.mockReset();
  let polls = 0;
  platform.fetch.mockReset().mockImplementation(async (url, options: any = {}) => {
    const address = String(url);
    if (address.includes("getAppAccessToken")) return response({ access_token: "token", expires_in: 7200 });
    if (address.endsWith("/gateway")) return response({ url: "wss://qq.example.test" });
    if (address.includes("gateway/connections/open")) return response({ endpoint: "wss://dingtalk.example.test", ticket: "ticket" });
    if (address.includes("oauth2")) return response(address.includes("api.dingtalk.io") ? { accessToken: "token", expireIn: 7200 } : { access_token: "token", expires_in: 7200 });
    if (address.includes("oToMessages/batchSend") || address.includes("groupMessages/send")) return response({ processQueryKey: "ding-receipt" });
    if (address.includes("ilink/bot/getupdates")) {
      if (++polls === 1) return response({ ret: 0, get_updates_buf: "buf", msgs: [{ message_id: 12345, from_user_id: "wx-sender", context_token: "wx-context", item_list: [{ type: 1, text_item: { text: "wx inbound" } }] }] });
      return new Promise((_resolve, reject) => options.signal?.addEventListener("abort", () => reject(new Error("aborted")), { once: true }));
    }
    if (address.includes("ilink/bot/sendmessage")) return response({ ret: 0, message_id: "wx-sent" });
    if (address.includes("ilink/bot/getconfig")) return response({ ret: 0, typing_ticket: "actual-typing-ticket" });
    if (address.includes("ilink/bot/sendtyping")) return response({ ret: 0 });
    if (address.includes("ilink/bot/getuploadurl")) return response({ ret: 0, upload_param: "upload-param" });
    if (address.includes("/c2c/upload")) return new Response("", { headers: { "x-encrypted-param": "download-param" } });
    if (address.endsWith("/upload_prepare")) return response({ upload_id: "upload-1", block_size: 1024, parts: [{ index: 1, presigned_url: "https://cos.example.test/part" }] });
    if (address.startsWith("https://cos.example.test/")) return new Response("");
    if (address.endsWith("/upload_part_finish")) return response({});
    if (address.endsWith("/files")) return response({ file_info: "native-file-info" });
    if (address.includes("api.sgroup.qq.com") && address.includes("/messages")) return response({ id: "qq-sent" });
    if (address.includes("api.telegram.org/file")) return new Response(Buffer.from("telegram-media"));
    throw new Error("Unexpected platform test HTTP request");
  });
  vi.stubGlobal("fetch", platform.fetch);
  vi.spyOn(console, "log").mockImplementation(() => {});
  vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.spyOn(console, "error").mockImplementation(() => {});
});
afterEach(async () => { await Promise.all(workers.splice(0).map(worker => worker.stop())); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

describe("R07 SDK worker 的真实五平台适配链", () => {
  it.each(["telegram", "feishu", "dingtalk", "qq", "wechat"] as const)("%s 确认发送只使用平台返回的编号，停用回收连接", async name => {
    const { worker, events } = await init(name);
    await send(worker, { replyContext: { isGroup: false, contextToken: name === "wechat" ? "wx-context" : null } });
    await vi.waitFor(() => expect(events.find(item => item.kind === "receipt")).toMatchObject({ status: "confirmed", platformMessageIds: [name === "telegram" ? "9001" : name === "feishu" ? "feishu-sent" : name === "dingtalk" ? "ding-receipt" : name === "qq" ? "qq-sent" : "wx-sent"] }));
    await worker.stop();
    expect(events.at(-1)).toMatchObject({ ...base, kind: "stopped" });
    if (name === "telegram") expect(platform.telegram[0].stopPolling).toHaveBeenCalledOnce();
    if (name === "feishu") expect(platform.closeFeishu).toHaveBeenCalledOnce();
    if (name === "qq" || name === "dingtalk") expect(platform.sockets[0].closed).toBe(true);
  });

  it("Telegram 保留真实消息/话题，附件先分片再交消息", async () => {
    const { worker, events } = await init("telegram");
    await platform.telegram[0].emit("message", { message_id: 88, message_thread_id: 21, chat: { id: -100, type: "supergroup" }, from: { id: 42, first_name: "Alice" }, caption: "caption", document: { file_id: "f", file_name: "note.txt", mime_type: "text/plain", file_size: 14 } });
    await vi.waitFor(() => expect(events.find(item => item.kind === "message")).toBeTruthy());
    const message = events.find(item => item.kind === "message");
    expect(message).toMatchObject({ messageId: "88", conversationId: "-100", senderId: "42", displayName: "Alice", conversationKind: "group", replyContext: { messageThreadId: 21, isGroup: true } });
    expect(events.findIndex(item => item.kind === "media_end")).toBeLessThan(events.indexOf(message));
    expect(events.find(item => item.kind === "media_begin")).toMatchObject({ messageId: "88", sizeBytes: 14 });
    expect(Buffer.from(events.find(item => item.kind === "media_chunk").data, "base64").toString()).toBe("telegram-media");
    await send(worker, { attachments: [], replyContext: message.replyContext });
    await vi.waitFor(() => expect(platform.sendTelegram).toHaveBeenCalled());
    expect(platform.sendTelegram.mock.calls[0][2]).toMatchObject({ message_thread_id: 21 });
  });

  it("飞书保留消息资源编号及线程上下文，下载走真实 SDK", async () => {
    const { events } = await init("feishu");
    await platform.handlers["im.message.receive_v1"]({ sender: { sender_type: "user", sender_id: { open_id: "ou-user" } }, message: { message_id: "om-native", chat_id: "oc-chat", chat_type: "p2p", message_type: "image", content: JSON.stringify({ image_key: "native-key" }), root_id: "om-root", parent_id: "om-parent" } });
    await vi.waitFor(() => expect(events.find(item => item.kind === "message")).toMatchObject({ messageId: "om-native", conversationId: "oc-chat", senderId: "ou-user", replyContext: { rootId: "om-root", parentId: "om-parent" } }));
    expect(platform.downloadFeishu).toHaveBeenCalledWith(expect.objectContaining({ path: { message_id: "om-native", file_key: "native-key" } }));
  });

  it("钉钉保留真实会话和消息编号，发送目标沿回调上下文", async () => {
    const { events } = await init("dingtalk");
    platform.sockets[0].emit("message", JSON.stringify({ type: "CALLBACK", headers: { topic: "/v1.0/im/bot/messages/get", messageId: "transport" }, data: JSON.stringify({ conversationId: "cid-native", conversationType: "1", msgId: "ding-native", senderStaffId: "staff", msgtype: "text", text: { content: "hello" } }) }));
    await vi.waitFor(() => expect(events.find(item => item.kind === "message")).toMatchObject({ messageId: "ding-native", conversationId: "cid-native", senderId: "staff", replyContext: { targetChatId: "staff", isGroup: false } }));
  });

  it("真实平台公开昵称和头像进入消息，头像不含凭据且不额外查询", async () => {
    const { events } = await init("dingtalk");
    platform.sockets[0].emit("message", JSON.stringify({ type: "CALLBACK", headers: { topic: "/v1.0/im/bot/messages/get", messageId: "transport" }, data: JSON.stringify({ conversationId: "dm", conversationType: "1", msgId: "profile-native", senderStaffId: "staff", senderNick: "平台昵称", senderAvatar: "https://cdn.example.test/public-avatar.png", msgtype: "text", text: { content: "hello" } }) }));
    await vi.waitFor(() => expect(events.find(item => item.kind === "message")).toMatchObject({ displayName: "平台昵称", avatarUrl: "https://cdn.example.test/public-avatar.png" }));
    expect(platform.fetch.mock.calls.every(([url]) => !String(url).includes("public-avatar"))).toBe(true);
  });

  it.each(["https://api.telegram.org/file/botnot-a-real-telegram-token/avatar.jpg", "javascript:alert(1)", "https://cdn.example.test/avatar?access_token=PRIVATE"])("不把私密或非法头像发给宿主：%s", async avatar => {
    const { events } = await init("telegram");
    platform.telegram[0].getUserProfilePhotos.mockResolvedValue({ photos: [[{ file_id: "avatar" }]] });
    platform.telegram[0].getFileLink.mockResolvedValue(avatar);
    await platform.telegram[0].emit("message", { message_id: 21, chat: { id: 2, type: "private" }, from: { id: 3, first_name: "可用昵称" }, text: "hello" });
    await vi.waitFor(() => expect(events.some(item => item.kind === "message")).toBe(true));
    expect(events.find(item => item.kind === "message")).toMatchObject({ displayName: "可用昵称" });
    expect(events.find(item => item.kind === "message")).not.toHaveProperty("avatarUrl");
    expect(JSON.stringify(events)).not.toContain(configs.telegram.token);
  });

  it("钉钉已有 legacy_app 的规范凭据可直接运行，无需恢复旧字段", async () => {
    const { worker, events } = await init("dingtalk", { agentId: "hana", authMode: "legacy_app", clientId: "legacy-app", clientSecret: "legacy-secret", robotCode: "robot" });
    await send(worker);
    await vi.waitFor(() => expect(events.find(item => item.kind === "receipt")).toMatchObject({ status: "confirmed", platformMessageIds: ["ding-receipt"] }));
    const token = platform.fetch.mock.calls.find(([url]) => String(url).endsWith("/oauth2/accessToken"));
    expect(JSON.parse(token[1].body)).toEqual({ appKey: "legacy-app", appSecret: "legacy-secret" });
  });

  it("QQ 频道私信保留 guildId，只向正式私信端点发送", async () => {
    const { worker, events } = await init("qq");
    platform.sockets[0].emit("message", JSON.stringify({ op: 0, t: "DIRECT_MESSAGE_CREATE", d: { id: "qq-native", guild_id: "guild", channel_id: "channel", author: { id: "qq-user" }, content: "hi" } }));
    await vi.waitFor(() => expect(events.find(item => item.kind === "message")).toMatchObject({ messageId: "qq-native", conversationId: "guild", senderId: "qq-user", replyContext: { guildId: "guild", channelId: "channel" } }));
    await send(worker, { replyContext: events.find(item => item.kind === "message").replyContext });
    await vi.waitFor(() => expect(events.find(item => item.kind === "receipt")).toMatchObject({ status: "confirmed" }));
    const sends = platform.fetch.mock.calls.filter(([url]) => String(url).includes("/messages"));
    expect(sends).toHaveLength(1); expect(String(sends[0][0])).toBe("https://api.sgroup.qq.com/dms/guild/messages");
  });

  it("微信保留真实编号和 contextToken，未返回编号的发送明确为未知", async () => {
    const { worker, events } = await init("wechat");
    await vi.waitFor(() => expect(events.find(item => item.kind === "message")).toMatchObject({ messageId: "12345", senderId: "wx-sender", replyContext: { contextToken: "wx-context" } }));
    platform.fetch.mockImplementationOnce(async () => response({ ret: 0 }));
    await send(worker, { replyContext: events.find(item => item.kind === "message").replyContext });
    await vi.waitFor(() => expect(events.find(item => item.kind === "receipt")).toMatchObject({ status: "unknown", platformMessageIds: [] }));
    const outbound = platform.fetch.mock.calls.find(([url]) => String(url).includes("ilink/bot/sendmessage"));
    expect(JSON.parse(outbound[1].body).msg.context_token).toBe("wx-context");
  });

  it("微信 uint64 消息及回执编号保持原始十进制，不经过浮点舍入", async () => {
    platform.fetch.mockImplementation(async (url, options: any = {}) => {
      if (String(url).includes("getupdates")) {
        if (!platform.fetch.mock.calls.slice(0, -1).some(([previous]) => String(previous).includes("getupdates")))
          return new Response('{"ret":0,"msgs":[{"message_id":18446744073709551614,"from_user_id":"wx-user","context_token":"ctx","item_list":[{"type":1,"text_item":{"text":"native"}}]}]}');
        return new Promise((_resolve, reject) => options.signal.addEventListener("abort", () => reject(new Error("aborted")), { once: true }));
      }
      if (String(url).includes("sendmessage")) return new Response('{"ret":0,"message_id":18446744073709551615}');
      throw new Error("unexpected HTTP");
    });
    const { worker, events } = await init("wechat");
    await vi.waitFor(() => expect(events.find(item => item.kind === "message")).toMatchObject({ messageId: "18446744073709551614" }));
    await send(worker, { replyContext: events.find(item => item.kind === "message").replyContext });
    await vi.waitFor(() => expect(events.find(item => item.kind === "receipt")).toMatchObject({ status: "confirmed", platformMessageIds: ["18446744073709551615"] }));
  });

  it.each(["feishu", "qq", "wechat"] as const)("%s 分片附件经过真实平台上传实现并取得发送回执", async name => {
    const { worker, events } = await init(name);
    await media(worker, "blob", Buffer.from("upload content"));
    await send(worker, { text: "", attachments: ["blob"], replyContext: { isGroup: false, contextToken: name === "wechat" ? "wx-context" : null } });
    await vi.waitFor(() => expect(events.find(item => item.kind === "receipt")).toMatchObject({ status: "confirmed" }));
    if (name === "qq") {
      const put = platform.fetch.mock.calls.find(([url]) => String(url).startsWith("https://cos.example.test/"));
      expect(put).toBeTruthy(); expect(await put[1].body.text()).toBe("upload content");
      const actual = platform.fetch.mock.calls.find(([url]) => String(url).endsWith("/messages"));
      expect(JSON.parse(actual[1].body).media.file_info).toBe("native-file-info");
    }
    if (name === "feishu") expect(platform.sendFeishu).toHaveBeenCalled();
    if (name === "wechat") expect(platform.fetch.mock.calls.some(([url]) => String(url).includes("/c2c/upload"))).toBe(true);
  });
});

describe("R07 持久授权效果的真实 SDK 边界", () => {
  it.each([undefined, new Error("waiting text acknowledgement unavailable")])("独立文字等待提示未确认不重发，也不阻止真实最终回复（%s）", async outcome => {
    const adapter = await createSdkAdapter("feishu", configs.feishu, {});
    // 验证非折叠能力的契约分支；创建、更新和回执仍使用正式飞书适配器。
    adapter.streamingCapabilities = { ...adapter.streamingCapabilities, receiptMode: "independent" };
    const effects = new SdkEffects("feishu", adapter, sdkReceiptIds, () => false);
    const input = { effectId: "waiting-1", streamId: "native-1", inboxId: "actual-inbox", runId: "actual-run", sequence: 1, operation: "receipt_start", deliveryId: null, conversationId: "dm", replyContext: { isGroup: false }, text: "正在准备回复…", rich: false };
    try {
      if (outcome instanceof Error) platform.sendFeishu.mockRejectedValueOnce(outcome);
      else platform.sendFeishu.mockResolvedValueOnce(outcome);
      expect(await effects.execute(input)).toMatchObject({ status: "unknown", platformAck: false });
      expect(await effects.execute({ ...input, effectId: "waiting-refresh", sequence: 2 })).toMatchObject({ status: "skipped", error: "waiting_text_already_sent" });
      expect(await effects.execute({ ...input, effectId: "actual-final", sequence: 3, operation: "stream_finish", deliveryId: "real-outbox", text: "最终回复" })).toMatchObject({ status: "confirmed", platformMessageIds: ["feishu-sent"] });
      expect(platform.sendFeishu).toHaveBeenCalledTimes(2);
      expect(platform.updateFeishu).toHaveBeenCalledOnce();
    } finally { await adapter.stop(); }
  });
  it("独立输入提示未确认不阻止同一范围内的真实最终回复", async () => {
    const { worker, events } = await init("telegram");
    platform.typingTelegram.mockRejectedValueOnce(new Error("typing response lost"));
    await effect(worker, "receipt_start", 1);
    expect(await effectReceipt(events, 1)).toMatchObject({ status: "unknown", platformAck: false });
    await effect(worker, "stream_finish", 2);
    expect(await effectReceipt(events, 2)).toMatchObject({ status: "confirmed", platformMessageIds: ["9001"] });
    expect(platform.sendTelegram).toHaveBeenCalledOnce();
  });
  it("Telegram 原生输入和富草稿不冒充最终送达，重复效果不重复外发", async () => {
    const { worker, events } = await init("telegram");
    expect(events.find(item => item.kind === "ready").capabilities).toMatchObject({ receipt: { mode: "native_typing" }, streams: expect.arrayContaining([{ mode: "rich_draft", scopes: ["dm"], minIntervalMs: 500, maxChars: 32768, rich: true, receiptFolded: false }]) });
    await effect(worker, "receipt_start", 1, { rich: true, replyContext: { isGroup: false, messageThreadId: 91 } });
    expect(await effectReceipt(events, 1)).toMatchObject({ status: "confirmed", platformAck: true, platformMessageIds: [] });
    expect(platform.typingTelegram).toHaveBeenCalledWith("conversation-1", "typing", { message_thread_id: 91 });
    const draft = { rich: true, replyContext: { isGroup: false, messageThreadId: 91 } };
    await effect(worker, "stream_update", 2, draft);
    expect(await effectReceipt(events, 2)).toMatchObject({ status: "confirmed", platformMessageIds: [] });
    await effect(worker, "stream_update", 2, draft);
    await vi.waitFor(() => expect(events.filter(item => item.kind === "effect_receipt" && item.receipt.sequence === 2)).toHaveLength(2));
    const firstDraft = platform.requestTelegram.mock.calls[0];
    expect(firstDraft[0]).toBe("sendRichMessageDraft");
    expect(firstDraft[1].form).toMatchObject({ chat_id: "conversation-1", message_thread_id: 91, draft_id: expect.any(Number) });
    expect(platform.requestTelegram).toHaveBeenCalledOnce();
    await effect(worker, "stream_finish", 3, draft);
    expect(await effectReceipt(events, 3)).toMatchObject({ operation: "stream_finish", deliveryId: "final-delivery", status: "confirmed", platformMessageIds: ["9002"] });
    expect(platform.requestTelegram.mock.calls.map(call => call[0])).toEqual(["sendRichMessageDraft", "sendRichMessage"]);
    expect(platform.sendTelegram).not.toHaveBeenCalled();
  });

  it.each([undefined, new Error("external error includes credential material")])("Telegram 草稿结果不明时，最终发送不能回落普通消息（%s）", async result => {
    const { worker, events } = await init("telegram");
    if (result instanceof Error) platform.requestTelegram.mockRejectedValueOnce(result);
    else platform.requestTelegram.mockResolvedValueOnce(result);
    await effect(worker, "stream_update", 1);
    expect(await effectReceipt(events, 1)).toMatchObject({ status: "unknown", platformAck: false, platformMessageIds: [] });
    await effect(worker, "stream_finish", 2);
    expect(await effectReceipt(events, 2)).toMatchObject({ status: "unknown", error: "stream_outcome_unconfirmed" });
    expect(platform.requestTelegram).toHaveBeenCalledOnce();
    expect(platform.sendTelegram).not.toHaveBeenCalled();
    expect(JSON.stringify(events)).not.toContain("credential material");
  });

  it("新进程不能根据旧序号重新创建不明状态的草稿", async () => {
    const { worker, events } = await init("telegram");
    await effect(worker, "stream_finish", 6);
    expect(await effectReceipt(events, 6)).toMatchObject({ status: "unknown", error: "stream_outcome_unconfirmed" });
    expect(platform.requestTelegram).not.toHaveBeenCalled(); expect(platform.sendTelegram).not.toHaveBeenCalled();
  });

  it("原生最终发送出错后保留未知，不改用普通发送也不重试外发", async () => {
    const { worker, events } = await init("telegram");
    await effect(worker, "stream_update", 1, { rich: true });
    expect(await effectReceipt(events, 1)).toMatchObject({ status: "confirmed" });
    platform.requestTelegram.mockRejectedValueOnce(new Error("platform request may have reached server"));
    await effect(worker, "stream_finish", 2, { rich: true });
    expect(await effectReceipt(events, 2)).toMatchObject({ status: "unknown", platformAck: false, platformMessageIds: [] });
    await effect(worker, "stream_finish", 2, { rich: true });
    await vi.waitFor(() => expect(events.filter(item => item.kind === "effect_receipt" && item.receipt.sequence === 2)).toHaveLength(2));
    await effect(worker, "stream_finish", 3, { rich: true });
    expect(await effectReceipt(events, 3)).toMatchObject({ status: "unknown", error: "stream_outcome_unconfirmed" });
    expect(platform.requestTelegram.mock.calls.map(call => call[0])).toEqual(["sendRichMessageDraft", "sendRichMessage"]);
    expect(platform.sendTelegram).not.toHaveBeenCalled();
  });

  it("宿主明确取消能清理结果不明的固定草稿，不能改成另一会话", async () => {
    const { worker, events } = await init("telegram");
    platform.requestTelegram.mockRejectedValueOnce(new Error("draft request timed out"));
    await effect(worker, "stream_update", 1);
    expect(await effectReceipt(events, 1)).toMatchObject({ status: "unknown" });
    await effect(worker, "stream_cancel", 2, { conversationId: "other-conversation" });
    await vi.waitFor(() => expect(events.some(item => item.kind === "error")).toBe(true));
    expect(platform.requestTelegram).toHaveBeenCalledOnce();
    await effect(worker, "stream_cancel", 2);
    expect(await effectReceipt(events, 2)).toMatchObject({ status: "confirmed", platformMessageIds: [] });
    expect(platform.requestTelegram.mock.calls[1][1].form).toMatchObject({ chat_id: "conversation-1", text: "", draft_id: platform.requestTelegram.mock.calls[0][1].form.draft_id });
    expect(platform.sendTelegram).not.toHaveBeenCalled();
  });

  it("飞书等待提示、逐步更新和最终回复复用真实消息编号", async () => {
    const { worker, events } = await init("feishu");
    await effect(worker, "receipt_start", 1, { text: "正在准备回复…" });
    expect(await effectReceipt(events, 1)).toMatchObject({ status: "confirmed", platformMessageIds: ["feishu-sent"] });
    await effect(worker, "stream_update", 2, { text: "逐步回复" });
    expect(await effectReceipt(events, 2)).toMatchObject({ status: "confirmed", platformMessageIds: ["feishu-sent"] });
    await effect(worker, "stream_finish", 3, { text: "最终回复" });
    expect(await effectReceipt(events, 3)).toMatchObject({ status: "confirmed", deliveryId: "final-delivery", platformMessageIds: ["feishu-sent"] });
    expect(platform.sendFeishu).toHaveBeenCalledOnce(); expect(platform.updateFeishu).toHaveBeenCalledTimes(2);
    for (const [request] of platform.updateFeishu.mock.calls) expect(request.path.message_id).toBe("feishu-sent");
  });

  it("飞书富回复使用真实 CardKit 创建、内容更新和关闭回执", async () => {
    const { worker, events } = await init("feishu");
    await effect(worker, "receipt_start", 1, { rich: true });
    expect(await effectReceipt(events, 1)).toMatchObject({ status: "confirmed", platformMessageIds: ["feishu-sent"] });
    await effect(worker, "stream_update", 2, { rich: true, text: "更新内容" });
    expect(await effectReceipt(events, 2)).toMatchObject({ status: "confirmed" });
    await effect(worker, "stream_finish", 3, { rich: true, text: "最终内容" });
    expect(await effectReceipt(events, 3)).toMatchObject({ status: "confirmed", platformMessageIds: ["feishu-sent"] });
    expect(platform.createFeishuCard).toHaveBeenCalledOnce(); expect(platform.sendFeishu).toHaveBeenCalledOnce();
    expect(JSON.parse(platform.sendFeishu.mock.calls[0][0].data.content)).toEqual({ type: "card", data: { card_id: "actual-card" } });
    expect(platform.contentFeishuCard).toHaveBeenCalledTimes(2); expect(platform.settingsFeishuCard).toHaveBeenCalledTimes(2);
    const settings = platform.settingsFeishuCard.mock.calls.map(([call]) => JSON.parse(call.data.settings));
    expect(settings).toEqual([{ config: { streaming_mode: true } }, { config: { streaming_mode: false } }]);
  });

  it("飞书缺少实际消息编号后不重新发送最终回复", async () => {
    const { worker, events } = await init("feishu"); platform.sendFeishu.mockResolvedValueOnce({ code: 0 });
    await effect(worker, "receipt_start", 1);
    expect(await effectReceipt(events, 1)).toMatchObject({ status: "unknown", platformMessageIds: [] });
    await effect(worker, "stream_finish", 2);
    expect(await effectReceipt(events, 2)).toMatchObject({ status: "unknown" });
    expect(platform.sendFeishu).toHaveBeenCalledOnce(); expect(platform.updateFeishu).not.toHaveBeenCalled();
  });

  it("飞书 CardKit 最终关闭未确认时不创建第二条最终气泡", async () => {
    const { worker, events } = await init("feishu");
    await effect(worker, "stream_update", 1, { rich: true });
    expect(await effectReceipt(events, 1)).toMatchObject({ status: "confirmed" });
    platform.settingsFeishuCard.mockRejectedValueOnce(new Error("close acknowledgement unavailable"));
    await effect(worker, "stream_finish", 2, { rich: true });
    expect(await effectReceipt(events, 2)).toMatchObject({ status: "unknown", platformAck: false });
    await effect(worker, "stream_finish", 3, { rich: true });
    expect(await effectReceipt(events, 3)).toMatchObject({ status: "unknown" });
    // 宿主先停止等待提示，再清理内容；前一步不能遗失未知卡片的真实句柄。
    await effect(worker, "receipt_stop", 4, { rich: true, text: "" });
    expect(await effectReceipt(events, 4)).toMatchObject({ status: "skipped" });
    await effect(worker, "stream_cancel", 5, { rich: true, text: "" });
    expect(await effectReceipt(events, 5)).toMatchObject({ status: "confirmed", platformMessageIds: ["feishu-sent"] });
    expect(JSON.parse(platform.settingsFeishuCard.mock.calls.at(-1)[0].data.settings)).toEqual({ config: { streaming_mode: false } });
    expect(platform.sendFeishu).toHaveBeenCalledOnce(); expect(platform.createFeishuCard).toHaveBeenCalledOnce();
  });

  it("飞书已创建卡片而流式设置超时，授权取消仍清理原卡片", async () => {
    const { worker, events } = await init("feishu");
    platform.settingsFeishuCard.mockRejectedValueOnce(new Error("settings outcome unknown"));
    await effect(worker, "stream_update", 1, { rich: true });
    expect(await effectReceipt(events, 1)).toMatchObject({ status: "unknown", platformMessageIds: ["feishu-sent"] });
    await effect(worker, "stream_cancel", 2, { rich: true, text: "" });
    expect(await effectReceipt(events, 2)).toMatchObject({ status: "confirmed", platformMessageIds: ["feishu-sent"] });
    expect(platform.createFeishuCard).toHaveBeenCalledOnce(); expect(platform.sendFeishu).toHaveBeenCalledOnce();
    const cleanup = platform.settingsFeishuCard.mock.calls.at(-1)[0];
    expect(cleanup.path.card_id).toBe("actual-card");
    expect(JSON.parse(cleanup.data.settings)).toEqual({ config: { streaming_mode: false } });
  });

  it("微信输入状态使用宿主冻结的上下文并支持真实取消", async () => {
    const { worker, events } = await init("wechat");
    const context = { isGroup: false, contextToken: "frozen-context-without-cache" };
    await effect(worker, "receipt_start", 1, { replyContext: context });
    expect(await effectReceipt(events, 1)).toMatchObject({ status: "confirmed", platformMessageIds: [] });
    expect(worker.lifecycleUsage.activeStreams).toBe(1);
    await effect(worker, "receipt_stop", 2, { replyContext: context });
    expect(await effectReceipt(events, 2)).toMatchObject({ status: "confirmed", platformMessageIds: [] });
    expect(worker.lifecycleUsage.activeStreams).toBe(0);
    const configCall = platform.fetch.mock.calls.find(([url]) => String(url).includes("ilink/bot/getconfig"));
    expect(JSON.parse(configCall[1].body).context_token).toBe(context.contextToken);
    const requests = platform.fetch.mock.calls.filter(([url]) => String(url).includes("ilink/bot/sendtyping")).map(([, options]) => JSON.parse(options.body));
    expect(requests.map(item => item.status)).toEqual([1, 2]);
    expect(requests.every(item => item.typing_ticket === "actual-typing-ticket")).toBe(true);
    expect(platform.fetch.mock.calls.some(([url]) => String(url).includes("ilink/bot/sendmessage"))).toBe(false);
  });

  it("微信结果不明的输入提示只允许取消原目标", async () => {
    const { worker, events } = await init("wechat");
    const original = platform.fetch.getMockImplementation()!;
    let failed = false;
    platform.fetch.mockImplementation(async (url, options) => {
      if (String(url).includes("ilink/bot/sendtyping") && !failed) { failed = true; throw new Error("typing acknowledgement missing"); }
      return original(url, options);
    });
    const replyContext = { isGroup: false, contextToken: "frozen-context" };
    await effect(worker, "receipt_start", 1, { replyContext });
    expect(await effectReceipt(events, 1)).toMatchObject({ status: "unknown" });
    await effect(worker, "receipt_stop", 2, { replyContext: { ...replyContext, targetChatId: "another-user" } });
    await vi.waitFor(() => expect(events.some(item => item.kind === "error")).toBe(true));
    expect(platform.fetch.mock.calls.filter(([url]) => String(url).includes("sendtyping"))).toHaveLength(1);
    await effect(worker, "receipt_stop", 2, { replyContext });
    expect(await effectReceipt(events, 2)).toMatchObject({ status: "confirmed", platformMessageIds: [] });
    const cleanup = platform.fetch.mock.calls.filter(([url]) => String(url).includes("sendtyping"))[1];
    expect(JSON.parse(cleanup[1].body)).toMatchObject({ ilink_user_id: "conversation-1", status: 2 });
  });

  it.each(["qq", "dingtalk"] as const)("%s 仅发送一次普通等待提示，不伪称支持草稿", async name => {
    const { worker, events } = await init(name);
    await effect(worker, "receipt_start", 1, { text: "等待提示" });
    expect(await effectReceipt(events, 1)).toMatchObject({ status: "confirmed", platformMessageIds: [name === "qq" ? "qq-sent" : "ding-receipt"] });
    await effect(worker, "receipt_start", 2, { text: "等待提示" });
    expect(await effectReceipt(events, 2)).toMatchObject({ status: "skipped", error: "waiting_text_already_sent" });
    await effect(worker, "stream_update", 3);
    expect(await effectReceipt(events, 3)).toMatchObject({ status: "skipped", platformAck: false, error: "native_streaming_scope_unsupported" });
    const outbound = platform.fetch.mock.calls.filter(([url]) => String(url).endsWith("/messages") || String(url).includes("oToMessages/batchSend"));
    expect(outbound).toHaveLength(1);
  });

  it("群聊不擅自启用私聊草稿，明确返回不支持", async () => {
    const { worker, events } = await init("telegram");
    await effect(worker, "stream_update", 1, { replyContext: { isGroup: true } });
    expect(await effectReceipt(events, 1)).toMatchObject({ status: "skipped", error: "native_streaming_scope_unsupported" });
    expect(platform.requestTelegram).not.toHaveBeenCalled();
  });

  it("效果字段与绑定严格封闭，退出不产生未经宿主授权的网络清理", async () => {
    const { worker, events } = await init("telegram");
    await effect(worker, "stream_update", 1, { role: "owner" });
    await vi.waitFor(() => expect(events.some(item => item.kind === "error")).toBe(true));
    expect(platform.requestTelegram).not.toHaveBeenCalled();
    await effect(worker, "stream_update", 1);
    expect(await effectReceipt(events, 1)).toMatchObject({ status: "confirmed" });
    await effect(worker, "stream_update", 2, { conversationId: "another-conversation" });
    await vi.waitFor(() => expect(events.filter(item => item.kind === "error")).toHaveLength(2));
    await worker.stop(); expect(platform.requestTelegram).toHaveBeenCalledOnce();
  });
});

describe("R07 SDK 可恢复连接状态", () => {
  it("钉钉临时断线保留正式重连，仅首连Ready且不泄漏SDK错误详情", async () => {
    const { worker, events } = await init("dingtalk");
    vi.useFakeTimers();
    try {
      const first = platform.sockets[0];
      first.emit("error", new Error("token=PRIVATE detail")); first.emit("close");
      await Promise.resolve(); await Promise.resolve();
      expect(events).toEqual(expect.arrayContaining([
        expect.objectContaining({ kind: "connection_status", status: "disconnected", error: "platform_connection_unavailable" }),
        expect.objectContaining({ kind: "connection_status", status: "connecting" }),
      ]));
      expect(events.some(event => event.kind === "error")).toBe(false);
      expect(JSON.stringify(events)).not.toContain("PRIVATE");
      await send(worker, { deliveryId: "admitted-before-disconnect" });
      await vi.advanceTimersByTimeAsync(1);
      expect(events.find(event => event.kind === "receipt")).toMatchObject({ deliveryId: "admitted-before-disconnect", status: "failed", platformMessageIds: [] });
      expect(platform.fetch.mock.calls.some(([url]) => String(url).includes("oToMessages/batchSend"))).toBe(false);
      await vi.advanceTimersByTimeAsync(5000);
      expect(platform.sockets).toHaveLength(2);
      platform.sockets[1].emit("open"); await Promise.resolve(); await Promise.resolve();
      expect(events.at(-1)).toMatchObject({ kind: "connection_status", status: "connected" });
      expect(events.filter(event => event.kind === "ready")).toHaveLength(1);
      await send(worker, { deliveryId: "new-after-reconnect" }); await vi.advanceTimersByTimeAsync(1);
      expect(events.at(-1)).toMatchObject({ kind: "receipt", deliveryId: "new-after-reconnect", status: "confirmed", platformMessageIds: ["ding-receipt"] });
      expect(events.some(event => event.kind === "error")).toBe(false);
    } finally { vi.useRealTimers(); }
  });
});

describe("R07 SDK 长期生命周期与递增重放边界", () => {
  const tick = () => new Promise<void>(resolve => setImmediate(resolve));
  function frame(dispatchSeq: number, index: number, operation = "stream_finish", sequence = 1) {
    return { ...base, kind: "effect", dispatchSeq, effect: { effectId: `long-effect-${dispatchSeq}`, streamId: `long-stream-${index}`, inboxId: `long-inbox-${index}`, runId: `long-run-${index}`, sequence, operation, deliveryId: operation === "stream_finish" ? `long-final-${index}` : null, conversationId: "dm", replyContext: { isGroup: false }, text: "实际回复", rich: false } };
  }
  it("超过 4096 次已完成发送和原生回复后仍有界，新任务继续成功，淘汰旧帧不能外发", async () => {
    const { worker, events } = await init("telegram");
    const firstEffect = frame(1, 0);
    const firstSend = { ...base, kind: "send", dispatchSeq: 2, deliveryId: "long-send-0", conversationId: "dm", text: "实际发送", attachments: [], replyContext: { isGroup: false } };
    let dispatchSeq = 0;
    for (let index = 0; index < 4200; index++) {
      await worker.accept(frame(++dispatchSeq, index)); await tick();
      expect(events.at(-1)).toMatchObject({ kind: "effect_receipt", receipt: { status: "confirmed" } });
      await worker.accept({ ...firstSend, dispatchSeq: ++dispatchSeq, deliveryId: `long-send-${index}` }); await tick();
      expect(events.at(-1)).toMatchObject({ kind: "receipt", status: "confirmed" });
      expect(worker.lifecycleUsage).toMatchObject({ activeStreams: 0, nativeHandles: 0 });
      for (const size of Object.values(worker.lifecycleUsage)) expect(size).toBeLessThanOrEqual(256);
      // 观测端同样不靠累积事件来维持防重。
      events.length = 0;
    }
    expect(platform.sendTelegram).toHaveBeenCalledTimes(8400);
    await worker.accept(firstEffect); await tick();
    await worker.accept(firstSend); await tick();
    expect(platform.sendTelegram).toHaveBeenCalledTimes(8400);
    expect(events.filter(event => event.kind === "error").map(event => event.error)).toEqual(["retired_dispatch_sequence", "retired_dispatch_sequence"]);
    // 旧 effect 的高序号同样不能凭新派发号创建新气泡。
    await worker.accept(frame(++dispatchSeq, 0, "stream_update", 2)); await tick();
    expect(events.at(-1)).toMatchObject({ kind: "effect_receipt", receipt: { status: "unknown", error: "stream_outcome_unconfirmed" } });
    expect(platform.sendTelegram).toHaveBeenCalledTimes(8400);
  }, 20_000);

  it("真正活跃的 128 条流保留预算，结束一条后可接新任务", async () => {
    const { worker, events } = await init("telegram");
    for (let index = 0; index < 128; index++) { await worker.accept(frame(index + 1, index, "stream_update")); await tick(); }
    expect(worker.lifecycleUsage.activeStreams).toBe(128);
    await worker.accept(frame(129, 128, "stream_update")); await tick();
    expect(events.at(-1)).toMatchObject({ kind: "error" });
    expect(platform.requestTelegram).toHaveBeenCalledTimes(128);
    await worker.accept(frame(130, 0, "stream_finish", 2)); await tick();
    expect(worker.lifecycleUsage.activeStreams).toBe(127);
    await worker.accept(frame(131, 129, "stream_update")); await tick();
    expect(events.at(-1)).toMatchObject({ kind: "effect_receipt", receipt: { status: "confirmed" } });
    expect(worker.lifecycleUsage.activeStreams).toBe(128);
  });

  it("同一活跃回复超过 4096 次已确认更新仍有界，完成后释放原生状态", async () => {
    const { worker, events } = await init("telegram");
    const first = frame(1, 0, "stream_update", 1);
    for (let sequence = 1; sequence <= 4200; sequence++) {
      await worker.accept(frame(sequence, 0, "stream_update", sequence)); await tick();
      expect(events.at(-1)).toMatchObject({ kind: "effect_receipt", receipt: { status: "confirmed" } });
      expect(worker.lifecycleUsage.activeStreams).toBe(1);
      expect(worker.lifecycleUsage.effectReceipts).toBeLessThanOrEqual(256);
      expect(worker.lifecycleUsage.dispatches).toBeLessThanOrEqual(256);
      events.length = 0;
    }
    await worker.accept(first); await tick();
    expect(events.at(-1)).toMatchObject({ kind: "error", error: "retired_dispatch_sequence" });
    expect(platform.requestTelegram).toHaveBeenCalledTimes(4200);
    await worker.accept(frame(4201, 0, "stream_finish", 4201)); await tick();
    expect(events.at(-1)).toMatchObject({ kind: "effect_receipt", receipt: { status: "confirmed" } });
    expect(worker.lifecycleUsage.activeStreams).toBe(0);
    expect(worker.lifecycleUsage.nativeHandles).toBe(0);
    expect(platform.sendTelegram).toHaveBeenCalledOnce();
  }, 20_000);

  it("正式发送与效果强制使用正整数派发序号，并拒绝同号改正文", async () => {
    const { worker, events } = await init("telegram");
    const command = { ...base, kind: "send", deliveryId: "d", conversationId: "dm", text: "正文", attachments: [], replyContext: { isGroup: false } };
    for (const dispatchSeq of [undefined, 0, 1.5, 2 ** 32]) { await worker.accept({ ...command, dispatchSeq }); }
    expect(events.filter(item => item.kind === "error")).toHaveLength(4);
    expect(platform.sendTelegram).not.toHaveBeenCalled();
    await worker.accept({ ...command, dispatchSeq: 1 }); await tick();
    await worker.accept({ ...command, dispatchSeq: 1, deliveryId: "other", text: "变更正文" }); await tick();
    expect(events.at(-1)).toMatchObject({ kind: "error", error: "dispatch_sequence_conflict" });
    expect(platform.sendTelegram).toHaveBeenCalledOnce();
  });
});

describe("R07 SDK worker 有界协议与退出", () => {
  it("严格限制初始化字段、代次、分片顺序及重放，拒绝任意路径和角色", async () => {
    const { worker, events } = await init("telegram");
    await worker.accept({ ...base, kind: "send", role: "owner" });
    await worker.accept({ ...base, generation: 8, kind: "stop" });
    await worker.accept({ ...base, kind: "media_begin", attachmentId: "x", sizeBytes: 3, path: "/secret" });
    await worker.accept({ ...base, kind: "media_chunk", attachmentId: "missing", seq: 1, data: "YWJj" });
    expect(events.filter(item => item.kind === "error").map(item => item.error)).toEqual(["unexpected_field", "connector_generation_mismatch", "unexpected_field", "invalid_media_chunk"]);
    await send(worker); await vi.waitFor(() => expect(events.some(item => item.kind === "receipt")).toBe(true)); await send(worker);
    expect(platform.sendTelegram).toHaveBeenCalledOnce(); expect(events.at(-1)).toMatchObject({ error: "delivery_not_admissible" });
    const invalid = new BridgeSdkWorker(async () => {});
    await expect(invalid.accept({ ...base, kind: "init", platform: "telegram", config: { ...configs.telegram, owner: "fake" } })).rejects.toThrow("unexpected_field");
  });

  it("媒体字节真实传给 SDK，附件从排队到完成始终计入限额", async () => {
    const { worker, events } = await init("telegram");
    let release: (value: any) => void;
    platform.sendTelegram.mockImplementationOnce(() => new Promise(resolve => { release = resolve; }));
    await media(worker, "asset", Buffer.from("owned bytes"));
    await send(worker, { text: "", attachments: ["asset"] });
    await vi.waitFor(() => expect(release).toBeTypeOf("function"));
    // 在途附件尚未结束，剩余七个空附件占满八项预算。
    for (let index = 0; index < 8; index++) await worker.accept({ ...base, kind: "media_begin", attachmentId: `queued-${index}`, sizeBytes: 0 });
    expect(events.at(-1)).toMatchObject({ error: "media_capacity_exhausted" });
    expect(platform.sendTelegram.mock.calls[0][1]).toEqual(Buffer.from("owned bytes"));
    release!({ message_id: 9002 });
    await vi.waitFor(() => expect(events.find(item => item.kind === "receipt")).toMatchObject({ status: "confirmed", platformMessageIds: ["9002"] }));
    await worker.accept({ ...base, kind: "media_begin", attachmentId: "after-finish", sizeBytes: 0 });
    expect(events.at(-1)).toMatchObject({ kind: "receipt" });
  });

  it("SDK 异常中的凭据不能进入事件或回执，停用不会新发消息", async () => {
    const { worker, events } = await init("telegram");
    platform.sendTelegram.mockRejectedValueOnce(new Error("https://api.telegram.org/botSECRET:token/sendMessage?contextToken=PRIVATE"));
    await send(worker);
    await vi.waitFor(() => expect(events.find(item => item.kind === "receipt")).toMatchObject({ status: "unknown", error: "platform_send_failed_or_unconfirmed" }));
    expect(JSON.stringify(events)).not.toMatch(/SECRET|PRIVATE/);
    await worker.stop(); await send(worker, { deliveryId: "delivery-2" });
    expect(platform.sendTelegram).toHaveBeenCalledOnce();
  });

  it("附件总预算、单项上限和平台地址在联网前被限制", async () => {
    const { worker, events } = await init("telegram");
    await worker.accept({ ...base, kind: "media_begin", attachmentId: "large", sizeBytes: SDK_WORKER_LIMITS.attachmentBytes });
    await worker.accept({ ...base, kind: "media_begin", attachmentId: "excess", sizeBytes: 15 * 1024 * 1024 });
    expect(events.at(-1)).toMatchObject({ error: "media_capacity_exhausted" });
    await worker.accept({ ...base, kind: "media_begin", attachmentId: "single-excess", sizeBytes: SDK_WORKER_LIMITS.attachmentBytes + 1 });
    expect(events.at(-1)).toMatchObject({ error: "media_capacity_exhausted" });
    platform.telegram[0].getFileLink.mockResolvedValueOnce("https://127.0.0.1/private");
    await platform.telegram[0].emit("message", { message_id: 99, chat: { id: 5, type: "private" }, from: { id: 42 }, document: { file_id: "f", file_size: 1 } });
    await vi.waitFor(() => expect(events.at(-1)).toMatchObject({ error: "untrusted_attachment_url" }));
    expect(platform.fetch).not.toHaveBeenCalled();
  });

  it("飞书下载流超量时中止，不向宿主发布不完整消息", async () => {
    const { events } = await init("feishu");
    const bytes = Buffer.alloc(1024 * 1024);
    platform.downloadFeishu.mockResolvedValueOnce({ getReadableStream: () => Readable.from(Array(51).fill(bytes)) });
    await platform.handlers["im.message.receive_v1"]({ sender: { sender_type: "user", sender_id: { open_id: "ou-user" } }, message: { message_id: "om-large", chat_id: "oc-chat", chat_type: "p2p", message_type: "image", content: JSON.stringify({ image_key: "large-key" }) } });
    await vi.waitFor(() => expect(events.at(-1)).toMatchObject({ error: "platform_operation_failed_or_unconfirmed" }));
    expect(events.some(item => item.kind === "message" || item.kind === "media_begin")).toBe(false);
  });

  it("钉钉未开放的媒体在任何发送前明确失败", async () => {
    const { worker, events } = await init("dingtalk");
    await media(worker, "blob", Buffer.from("owned"));
    await send(worker, { text: "", attachments: ["blob"] });
    await vi.waitFor(() => expect(events.find(item => item.kind === "receipt")).toMatchObject({ status: "failed", error: "platform_media_unsupported" }));
    expect(platform.fetch.mock.calls.some(([url]) => String(url).includes("Messages"))).toBe(false);
  });

  it("管道支持半行分段，正常 EOF 和信号均等待真实连接回收", async () => {
    const input = new PassThrough(); const output = new PassThrough(); const events: any[] = [];
    output.on("data", bytes => events.push(...String(bytes).trim().split("\n").map(line => JSON.parse(line))));
    const signals = new EventEmitter();
    const done = runBridgeSdkWorker(input, output, signals as any);
    const initLine = JSON.stringify({ ...base, kind: "init", platform: "telegram", config: configs.telegram });
    input.write(initLine.slice(0, 12)); input.write(`${initLine.slice(12)}\n`);
    await vi.waitFor(() => expect(events.some(item => item.kind === "ready")).toBe(true));
    let release: () => void;
    platform.telegram[0].stopPolling.mockImplementationOnce(() => new Promise<void>(resolve => { release = resolve; }));
    signals.emit("SIGTERM");
    await vi.waitFor(() => expect(release).toBeTypeOf("function"));
    expect(events.some(item => item.kind === "stopped")).toBe(false);
    release!(); await done;
    expect(events.at(-1).kind).toBe("stopped"); expect(signals.listenerCount("SIGTERM")).toBe(0);
    await expect(runBridgeSdkWorker(Readable.from([`${initLine}\n`]), new Writable({ write(_bytes, _encoding, callback) { callback(); } }), new EventEmitter() as any)).resolves.toBeUndefined();
  });

  it.each([Buffer.from("{"), Buffer.alloc(SDK_WORKER_LIMITS.lineBytes + 1, 65), Buffer.from([255, 10])])("拒绝截断、超长或非法 UTF8 输入并回收", async input => {
    await expect(runBridgeSdkWorker(Readable.from([input]), new Writable({ write(_bytes, _encoding, callback) { callback(); } }), new EventEmitter() as any)).rejects.toThrow();
  });

  it("原生 Node 入口可直接启动，非法输入只输出稳定错误", async () => {
    const child = spawn(process.execPath, ["scripts/bridge-sdk-worker.ts"], { cwd: process.cwd(), stdio: ["pipe", "pipe", "pipe"] });
    let stdout = ""; let stderr = "";
    child.stdout.on("data", value => { stdout += value; }); child.stderr.on("data", value => { stderr += value; });
    const closed = new Promise<number>(resolve => child.on("close", resolve));
    child.stdin.end('{"kind":"init","config":{"token":"MUST_NOT_LEAK"}}\n');
    expect(await closed).toBe(1); expect(stdout).toBe(""); expect(stderr).toBe("bridge_sdk_worker_failed\n");
  });

  it("回执识别绝不使用自造 client_id 或部分成功填补未知", () => {
    expect(sdkReceiptIds("wechat", { ret: 0, client_id: "generated" })).toBeNull();
    expect(sdkReceiptIds("telegram", [{ message_id: 1 }, {}])).toBeNull();
    expect(sdkReceiptIds("dingtalk", { processQueryKey: "query", invalidUserIdList: ["bad"] })).toBeNull();
    expect(sdkReceiptIds("telegram", { message_id: Number.MAX_SAFE_INTEGER + 1 })).toBeNull();
  });
});

describe("R07 单次诊断真实平台调用", () => {
  it.each(["telegram", "feishu", "dingtalk", "qq", "wechat"] as const)("%s 验证调用真实供应商入口，不启动接收循环", async name => {
    platform.fetch.mockImplementation(async (url) => {
      const address = String(url);
      if (address.includes("tenant_access_token")) return response({ code: 0, tenant_access_token: "tenant-test-token" });
      if (address.includes("oauth2")) return response({ access_token: "ding-test-token", expires_in: 7200 });
      if (address.includes("getAppAccessToken")) return response({ access_token: "qq-test-token" });
      if (address.endsWith("/users/@me")) return response({ id: "qq-real", username: "QQ test" });
      if (address.includes("getconfig")) return response({ ret: 0 });
      throw new Error("诊断不应启动长连接");
    });
    const result = await runSdkDiagnostic({ kind: "verify", platform: name, config: configs[name] });
    expect(result).toMatchObject({ kind: "diagnostic_result", ok: true, info: { credentialOk: true } });
    expect(platform.sockets).toHaveLength(0);
    expect(platform.fetch.mock.calls.every(([url]) => !/getupdates|gateway/.test(String(url)))).toBe(true);
    if (name === "telegram") { expect(platform.telegram[0].stopPolling).toHaveBeenCalledOnce(); expect(platform.sendTelegram).not.toHaveBeenCalled(); }
    expect(JSON.stringify(result)).not.toContain("test-token");
  });

  it("二维码和确认状态复用现役登录函数，凭据只随确认结果返回", async () => {
    platform.fetch.mockResolvedValueOnce(response({ qrcode: "qr-native", qrcode_img_content: "https://weixin.qq.com/login/test" }));
    const qr = await runSdkDiagnostic({ kind: "wechat_qrcode" });
    expect(qr).toMatchObject({ ok: true, qrcodeId: "qr-native" });
    expect(qr.qrcodeUrl).toMatch(/^data:image\/png;base64,/);
    platform.fetch.mockResolvedValueOnce(response({ status: "scaned" }));
    expect(await runSdkDiagnostic({ kind: "wechat_qrcode_status", qrcodeId: "qr-native" })).toEqual({ kind: "diagnostic_result", ok: true, status: "scanned" });
    platform.fetch.mockResolvedValueOnce(response({ status: "confirmed", bot_token: "owner-only-token", ilink_bot_id: "bot-native", ilink_user_id: "user-native", baseurl: "https://ilinkai.weixin.qq.com" }));
    expect(await runSdkDiagnostic({ kind: "wechat_qrcode_status", qrcodeId: "qr-native" })).toMatchObject({ ok: true, status: "confirmed", botToken: "owner-only-token", botId: "bot-native", userId: "user-native" });
    expect(platform.sockets).toHaveLength(0);
  });

  it("诊断拒绝多余字段、任意地址和泄漏原始平台错误", async () => {
    expect(await runSdkDiagnostic({ kind: "verify", platform: "telegram", config: { ...configs.telegram, owner: "fake" } })).toMatchObject({ ok: false });
    expect(await runSdkDiagnostic({ kind: "verify", platform: "wechat", config: { ...configs.wechat, baseUrl: "http://127.0.0.1" } })).toMatchObject({ ok: false });
    platform.fetch.mockRejectedValueOnce(new Error("token=SECRET&contextToken=PRIVATE"));
    expect(await runSdkDiagnostic({ kind: "wechat_qrcode" })).toEqual({ kind: "diagnostic_result", ok: false, error: "diagnostic_request_failed" });
    const output: Buffer[] = [];
    await runBridgeSdkDiagnostic(Readable.from(['{"kind":"wechat_qrcode"}\n{"kind":"wechat_qrcode"}\n']), new Writable({ write(bytes, _encoding, callback) { output.push(bytes); callback(); } }));
    expect(JSON.parse(Buffer.concat(output).toString())).toMatchObject({ ok: false, error: "diagnostic_request_invalid" });
  });

  it("原生 Node 诊断入口单次退出且无额外日志", async () => {
    const child = spawn(process.execPath, ["scripts/bridge-sdk-diagnostic.ts"], { cwd: process.cwd(), stdio: ["pipe", "pipe", "pipe"] });
    let stdout = ""; let stderr = "";
    child.stdout.on("data", bytes => { stdout += bytes; }); child.stderr.on("data", bytes => { stderr += bytes; });
    const closed = new Promise<number>(resolve => child.on("close", resolve));
    child.stdin.end('{"kind":"verify","platform":"telegram","config":{"token":"SECRET","owner":"fake"}}\n');
    expect(await closed).toBe(0); expect(stderr).toBe("");
    expect(JSON.parse(stdout)).toEqual({ kind: "diagnostic_result", ok: false, error: "diagnostic_request_failed" });
  });
});
