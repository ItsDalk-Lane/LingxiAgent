/** 单连接平台外围：只消费宿主授权的配置、正文和分片字节，不装载会话或模型。 */
import { createHash } from "node:crypto";
import { mkdtemp, writeFile, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import type { Readable, Writable } from "node:stream";
import { SdkEffects } from "./sdk-effects.ts";
import { SDK_RECENT_DISPATCHES, SdkDispatchWindow } from "./sdk-dispatch-window.ts";
import type { BridgeConnectionStatus, BridgeWorkerCommand, BridgeWorkerEvent } from "../../contracts/generated/ts/lingxi-protocol.ts";

export const SDK_WORKER_LIMITS = Object.freeze({ lineBytes: 256 * 1024, chunkBytes: 48 * 1024, attachmentBytes: 50 * 1024 * 1024, cachedBytes: 64 * 1024 * 1024, attachments: 8, operations: 32, textBytes: 100_000 });
export type Platform = "telegram" | "feishu" | "dingtalk" | "qq" | "wechat";
type Json = Record<string, any>;
type Emit = (event: BridgeWorkerEvent) => Promise<void>;
type WithoutIdentity<T> = T extends unknown ? Omit<T, "connectorId" | "generation"> : never;
type EventPayload = WithoutIdentity<BridgeWorkerEvent>;
type Media = { size: number; mime: string; filename: string; chunks: Buffer[]; received: number; seq: number; complete: boolean };
class Invalid extends Error {}
function object(value: unknown): Json { if (!value || typeof value !== "object" || Array.isArray(value)) throw new Invalid("object_required"); return value as Json; }
function keys(value: Json, allowed: string[]) { if (Object.keys(value).some(key => !allowed.includes(key))) throw new Invalid("unexpected_field"); }
function text(value: unknown, max = 1024, empty = false): string { if (typeof value !== "string" || (!empty && !value.trim()) || Buffer.byteLength(value) > max || value.includes("\0")) throw new Invalid("invalid_text"); return value; }
function uint(value: unknown): number { if (!Number.isInteger(value) || Number(value) < 0 || Number(value) > 0xffff_ffff) throw new Invalid("invalid_u32"); return Number(value); }
function nativeId(value: unknown): string { return text(typeof value === "number" && Number.isSafeInteger(value) ? String(value) : value); }
function replyContext(value: unknown): Json {
  if (value == null) return {};
  const result = object(value);
  keys(result, ["messageId", "text", "senderId", "messageThreadId", "contextToken", "targetChatId", "isGroup", "targetType", "rootId", "parentId", "guildId", "channelId"]);
  for (const [key, val] of Object.entries(result)) {
    if (val == null) continue;
    if (key === "isGroup") { if (typeof val !== "boolean") throw new Invalid("invalid_scope"); }
    else if (key === "messageThreadId") uint(val);
    else text(val, key === "text" ? 8192 : 16_384, true);
  }
  if (result.targetType != null && !["user", "group", "channel"].includes(result.targetType)) throw new Invalid("invalid_target_type");
  return { ...result };
}
export function sdkCredentials(platform: Platform, input: unknown): Json {
  const original = object(input);
  const config = Object.fromEntries(Object.entries(original).filter(([, value]) => value != null));
  const fields = { telegram: ["token"], feishu: ["appId", "appSecret", "region"], dingtalk: ["authMode", "corpId", "clientId", "clientSecret", "appKey", "appSecret", "robotCode", "apiBaseUrl", "restBaseUrl", "streamOpenUrl"], qq: ["appID", "appId", "appSecret", "secret"], wechat: ["botToken", "token", "baseUrl", "apiBaseUrl", "accountId"] }[platform];
  if (!fields) throw new Invalid("unknown_platform");
  keys(original, ["agentId", ...fields]);
  keys(config, ["agentId", ...fields]);
  text(config.agentId);
  for (const value of Object.values(config)) text(value, 16_384);
  if (platform === "qq") {
    config.appID = Object.hasOwn(config, "appID") ? config.appID : config.appId;
    config.appSecret = Object.hasOwn(config, "appSecret") ? config.appSecret : config.secret;
    delete config.appId; delete config.secret;
  }
  if (platform === "wechat") {
    config.botToken = Object.hasOwn(config, "botToken") ? config.botToken : config.token;
    delete config.token;
    if (config.baseUrl || config.apiBaseUrl) {
      const url = new URL(config.baseUrl || config.apiBaseUrl);
      if (url.origin !== "https://ilinkai.weixin.qq.com" || url.pathname !== "/" || url.search || url.hash || url.username || url.password) throw new Invalid("untrusted_platform_endpoint");
      config.baseUrl = url.origin;
    }
    delete config.apiBaseUrl; delete config.accountId;
  }
  if (platform === "dingtalk") {
    for (const key of ["apiBaseUrl", "restBaseUrl", "streamOpenUrl"]) {
      const allowed = key === "streamOpenUrl" ? ["https://api.dingtalk.com/v1.0/gateway/connections/open"] : ["https://api.dingtalk.com/v1.0", "https://api.dingtalk.io/v1.0"];
      if (config[key] && !allowed.includes(config[key].replace(/\/$/, ""))) throw new Invalid("untrusted_platform_endpoint");
    }
    text(Object.hasOwn(config, "clientId") ? config.clientId : config.appKey, 16_384);
    text(Object.hasOwn(config, "clientSecret") ? config.clientSecret : config.appSecret, 16_384);
    if (config.authMode !== "legacy_app") text(config.corpId, 16_384);
  }
  const required = { telegram: ["token"], feishu: ["appId", "appSecret"], dingtalk: ["robotCode"], qq: ["appID", "appSecret"], wechat: ["botToken"] }[platform];
  for (const field of required) text(config[field], 16_384);
  return { ...config };
}

/** 生产入口与测试共用真实工厂；测试只能替换各平台SDK/HTTP响应。 */
export async function createSdkAdapter(platform: Platform, config: Json, hooks: Json): Promise<any> {
  const options: any = { ...config, ...hooks };
  switch (platform) {
    case "telegram": return (await import("./telegram-adapter.ts")).createTelegramAdapter(options);
    case "feishu": return (await import("./feishu-adapter.ts")).createFeishuAdapter(options);
    case "dingtalk": return (await import("./dingtalk-adapter.ts")).createDingTalkAdapter(options);
    case "qq": return (await import("./qq-adapter.ts")).createQQAdapter(options);
    case "wechat": return (await import("./wechat-adapter.ts")).createWechatAdapter({ ...options, lingxiHome: "" });
  }
}

/** 只有真实返回的消息/平台查询凭据能确认成功；空结果不能补造编号。 */
export function sdkReceiptIds(platform: Platform, result: any): string[] | null {
  if (Array.isArray(result)) {
    if (!result.length) return null;
    const parts = result.map(value => sdkReceiptIds(platform, value));
    return parts.every(Boolean) ? parts.flat() as string[] : null;
  }
  if (!result || typeof result !== "object") return null;
  const id = platform === "telegram" ? result.message_id
    : platform === "feishu" ? result.data?.message_id
    : platform === "dingtalk" ? result.processQueryKey
    : platform === "qq" ? result.id
    : result.message_id ?? result.msg_id;
  if (result.invalidUserIdList?.length) return null;
  try { return [nativeId(id)]; } catch { return null; }
}

export class BridgeSdkWorker {
  private identity: { connectorId: string; generation: number; platform: Platform } | null = null;
  private adapter: any = null;
  private effects: SdkEffects | null = null;
  private resolveEffectsReady!: () => void;
  private effectsReady = new Promise<void>(resolve => { this.resolveEffectsReady = resolve; });
  private phase: "new" | "connecting" | "ready" | "stopping" | "stopped" = "new";
  private connected = false;
  private lastConnection: BridgeConnectionStatus | null = null;
  private media = new Map<string, Media>();
  private reservedBytes = 0;
  private reservedAttachments = 0;
  private tasks = new Set<Promise<unknown>>();
  private sends: Promise<unknown> = Promise.resolve();
  private inbound: Promise<unknown> = Promise.resolve();
  private deliveries = new Map<string, boolean>();
  private dispatches = new SdkDispatchWindow<EventPayload>();
  private secrets: string[] = [];
  private abort = new AbortController();
  private closing: Promise<void> | null = null;
  private readonly emit: Emit;
  constructor(emit: Emit) { this.emit = emit; }
  get stopped() { return this.phase === "stopping" || this.phase === "stopped"; }
  get lifecycleUsage() { return { dispatches: this.dispatches.size, deliveries: this.deliveries.size, ...this.effects?.lifecycleUsage }; }
  private async event(event: EventPayload) { if (this.identity) await this.emit({ ...event, connectorId: this.identity.connectorId, generation: this.identity.generation }); }
  private track(task: Promise<unknown>) {
    this.tasks.add(task);
    void task.finally(() => this.tasks.delete(task)).catch(() => {});
  }
  private async failure(error: unknown) { await this.event({ kind: "error", error: error instanceof Invalid ? error.message : "platform_operation_failed_or_unconfirmed" }); }
  private async ready() {
    await this.effectsReady; if (!this.effects || this.stopped) return;
    if (this.phase === "connecting") { this.phase = "ready"; this.connected = true; this.lastConnection = "connected"; await this.event({ kind: "ready", capabilities: this.effects.capabilities }); }
  }
  private async connection(status: BridgeConnectionStatus) {
    this.connected = status === "connected";
    await this.effectsReady; if (!this.effects || this.stopped) return;
    if (status === "connected" && this.phase === "connecting") { await this.ready(); return; }
    if (this.lastConnection === status) return;
    this.lastConnection = status;
    await this.event({ kind: "connection_status", status, ...(status === "disconnected" ? { error: "platform_connection_unavailable" } : {}) });
  }
  private async dispatch(command: Json, execute: () => Promise<EventPayload>) {
    let admission;
    try { admission = this.dispatches.begin(command.dispatchSeq, command); }
    catch (error) { throw new Invalid((error as Error).message); }
    if (!admission.fresh) { if (admission.entry.done && admission.entry.result) await this.event(admission.entry.result); return false; }
    const task = this.sends.then(async () => {
      let result: EventPayload;
      try { result = await execute(); }
      catch { result = { kind: "error", error: "platform_operation_failed_or_unconfirmed" }; }
      this.dispatches.complete(admission.entry, result); await this.event(result);
    });
    this.sends = task.catch(() => {}); this.track(task);
    return true;
  }
  private rememberDelivery(id: string, terminal: boolean) {
    this.deliveries.set(id, terminal);
    for (const [old, done] of this.deliveries) { if (this.deliveries.size <= SDK_RECENT_DISPATCHES) break; if (done) this.deliveries.delete(old); }
  }

  async accept(input: unknown): Promise<void> {
    try {
      const command = object(input);
      text(command.kind, 32);
      const base = ["kind", "connectorId", "generation"];
      const fields: { [K in BridgeWorkerCommand["kind"]]: (keyof Extract<BridgeWorkerCommand, { kind: K }>)[] } = { init: ["platform", "config"], send: ["dispatchSeq", "deliveryId", "conversationId", "text", "replyContext", "attachments"], media_begin: ["attachmentId", "messageId", "sizeBytes", "mime", "filename"], media_chunk: ["attachmentId", "seq", "data"], media_end: ["attachmentId"], effect: ["dispatchSeq", "effect"], stop: [] };
      if (!Object.hasOwn(fields, command.kind)) throw new Invalid("unknown_command");
      keys(command, [...base, ...fields[command.kind]]);
      const connectorId = text(command.connectorId);
      const generation = uint(command.generation);
      if (command.kind === "init") {
        if (this.phase !== "new") throw new Invalid("already_initialized");
        if (!["telegram", "feishu", "dingtalk", "qq", "wechat"].includes(command.platform)) throw new Invalid("unknown_platform");
        const config = sdkCredentials(command.platform, command.config);
        this.secrets = Object.entries(config).filter(([key, value]) => /token|secret|appkey|clientid|appid|corpid/i.test(key) && typeof value === "string").map(([, value]) => value);
        this.identity = { connectorId, generation, platform: command.platform };
        this.phase = "connecting";
        this.adapter = await createSdkAdapter(command.platform, config, {
          onMessage: (message: Json) => {
            if (this.stopped) return;
            if (this.tasks.size >= SDK_WORKER_LIMITS.operations) { void this.failure(new Invalid("inbound_capacity_exhausted")).finally(() => this.stop()).catch(() => {}); return; }
            const task = this.inbound.then(() => this.receive(message)).catch(error => this.failure(error));
            this.inbound = task;
            this.track(task);
          },
          onStatus: (status: string) => {
            if (this.stopped) return;
            if (this.tasks.size >= SDK_WORKER_LIMITS.operations) { void this.stop().catch(() => {}); return; }
            const task = status === "connected" ? this.connection("connected") : status === "connecting" ? this.connection("connecting") : status === "error" || status === "disconnected" ? this.connection("disconnected") : Promise.resolve();
            this.track(task);
          },
        });
        this.effects = new SdkEffects(command.platform, this.adapter, sdkReceiptIds, () => this.stopped || !this.connected);
        this.resolveEffectsReady();
        if (command.platform === "telegram") this.track(Promise.resolve().then(() => this.adapter.getMe()).then(me => { nativeId(me?.id); return this.connection("connected"); }).catch(error => this.failure(error)));
        return;
      }
      if (!this.identity || connectorId !== this.identity.connectorId || generation !== this.identity.generation) throw new Invalid("connector_generation_mismatch");
      if (command.kind === "stop") { await this.stop(); return; }
      if (this.stopped) throw new Invalid("worker_stopped");
      if (command.kind === "effect") {
        const effect = object(command.effect);
        replyContext(effect.replyContext);
        if (this.phase !== "ready" || !this.effects || this.tasks.size >= SDK_WORKER_LIMITS.operations) throw new Invalid("effect_not_admissible");
        await this.dispatch(command, async () => ({ kind: "effect_receipt", receipt: await this.effects!.execute(effect) })); return;
      }
      if (command.kind === "media_begin") {
        const id = text(command.attachmentId);
        const size = uint(command.sizeBytes);
        if (command.messageId != null) text(command.messageId);
        const mime = command.mime == null ? "application/octet-stream" : text(command.mime, 256);
        const filename = command.filename == null ? "attachment" : text(command.filename, 1024);
        if (this.media.has(id) || this.reservedAttachments >= SDK_WORKER_LIMITS.attachments || size > SDK_WORKER_LIMITS.attachmentBytes || this.reservedBytes + size > SDK_WORKER_LIMITS.cachedBytes) throw new Invalid("media_capacity_exhausted");
        this.media.set(id, { size, mime, filename, chunks: [], received: 0, seq: 0, complete: false });
        this.reservedBytes += size;
        this.reservedAttachments++;
      } else if (command.kind === "media_chunk") {
        const media = this.media.get(text(command.attachmentId));
        const data = text(command.data, SDK_WORKER_LIMITS.chunkBytes * 4 / 3, true);
        if (!media || media.complete || uint(command.seq) !== media.seq || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(data)) throw new Invalid("invalid_media_chunk");
        const bytes = Buffer.from(data, "base64");
        if (!bytes.length || bytes.toString("base64") !== data || media.received + bytes.length > media.size) throw new Invalid("media_size_mismatch");
        media.chunks.push(bytes); media.received += bytes.length; media.seq++;
      } else if (command.kind === "media_end") {
        const media = this.media.get(text(command.attachmentId));
        if (!media || media.complete || media.received !== media.size) throw new Invalid("media_size_mismatch");
        media.complete = true;
      } else {
        const deliveryId = text(command.deliveryId);
        const conversationId = text(command.conversationId);
        const content = text(command.text, SDK_WORKER_LIMITS.textBytes, true);
        const context = replyContext(command.replyContext);
        if (typeof context.isGroup !== "boolean") throw new Invalid("explicit_conversation_scope_required");
        if (!Array.isArray(command.attachments) || command.attachments.length > SDK_WORKER_LIMITS.attachments || new Set(command.attachments).size !== command.attachments.length) throw new Invalid("invalid_attachments");
        const attachments = command.attachments.map(id => { text(id); const media = this.media.get(id); if (!media?.complete) throw new Invalid("attachment_not_complete"); return { id, media }; });
        if ((!content && !attachments.length) || this.phase !== "ready" || this.tasks.size >= SDK_WORKER_LIMITS.operations || this.deliveries.has(deliveryId)) throw new Invalid("delivery_not_admissible");
        // 先验证递增授权边界，随后才从媒体缓存移交字节；退役旧帧不能消耗新附件。
        const fresh = await this.dispatch(command, async () => {
          try { return await this.send(deliveryId, conversationId, content, context, attachments.map(item => item.media)); }
          finally { this.rememberDelivery(deliveryId, true); }
        });
        if (!fresh) return;
        this.rememberDelivery(deliveryId, false);
        // 排队发送仍占用预算，直到真正交付结束才释放，避免慢平台积累大量附件。
        for (const { id } of attachments) this.media.delete(id);
      }
    } catch (error) { if (!this.identity) throw error; await this.failure(error); }
  }

  private async send(deliveryId: string, conversationId: string, content: string, context: Json, attachments: Media[]): Promise<EventPayload> {
    const platform = this.identity!.platform;
    const ids: string[] = [];
    let known = true;
    let invoked = false;
    let temp: string | null = null;
    try {
      if (this.stopped || !this.connected) throw new Invalid("worker_unavailable_before_send");
      const target = context.targetChatId == null ? conversationId : text(context.targetChatId);
      const options = { ...context, replyContext: context, targetType: context.targetType || (context.isGroup ? "group" : "user") };
      if (content) { invoked = true; const response = await this.adapter.sendReply(target, content, options); const actual = sdkReceiptIds(platform, response); if (actual) ids.push(...actual); else known = false; }
      for (const media of attachments) {
        if (this.stopped || !this.connected) throw new Invalid("worker_unavailable_during_send");
        const buffer = Buffer.concat(media.chunks, media.size);
        const metadata = { ...options, mime: media.mime, filename: media.filename };
        let response;
        if (platform === "qq") {
          if (context.guildId || context.targetType === "channel") throw new Invalid("platform_target_media_unsupported");
          temp ??= await mkdtemp(path.join(os.tmpdir(), "lingxi-bridge-worker-"));
          const file = path.join(temp, "attachment");
          await writeFile(file, buffer, { mode: 0o600 });
          invoked = true;
          response = await this.adapter.sendMediaFile(target, file, metadata);
        } else {
          if (typeof this.adapter.sendMediaBuffer !== "function") throw new Invalid("platform_media_unsupported");
          invoked = true;
          response = await this.adapter.sendMediaBuffer(target, buffer, metadata);
        }
        const actual = sdkReceiptIds(platform, response); if (actual) ids.push(...actual); else known = false;
      }
      return { kind: "receipt", deliveryId, status: known ? "confirmed" : "unknown", platformMessageIds: ids, error: known ? null : "platform_did_not_return_all_message_ids" };
    } catch (error) {
      return { kind: "receipt", deliveryId, status: invoked ? "unknown" : "failed", platformMessageIds: ids, error: error instanceof Invalid ? error.message : "platform_send_failed_or_unconfirmed" };
    } finally {
      for (const media of attachments) { this.reservedBytes -= media.size; this.reservedAttachments--; }
      if (temp) await rm(temp, { recursive: true, force: true });
    }
  }

  private async receive(message: Json) {
    if (this.stopped) return;
    const messageId = nativeId(message.messageId ?? message._msgId);
    const conversationId = nativeId(message.conversationId ?? message.chatId);
    const senderId = nativeId(message.userId ?? message.principalId);
    const content = text(message.text || "", SDK_WORKER_LIMITS.textBytes, true);
    const context = replyContext({ ...message.replyContext, messageId, senderId, isGroup: message.isGroup === true, targetChatId: nativeId(message.chatId), ...(message.messageThreadId != null ? { messageThreadId: message.messageThreadId } : {}), ...(message.replyTargetType ? { targetType: message.replyTargetType } : {}) });
    const source = message.attachments || [];
    if (!Array.isArray(source) || source.length > SDK_WORKER_LIMITS.attachments) throw new Invalid("invalid_inbound_attachments");
    await this.ready();
    const attachments = [];
    let total = 0;
    for (let index = 0; index < source.length; index++) {
      const item = object(source[index]);
      const buffer = await this.download(item, messageId);
      total += buffer.length;
      if (total > SDK_WORKER_LIMITS.cachedBytes) throw new Invalid("inbound_media_capacity_exhausted");
      const attachmentId = `sdk-${createHash("sha256").update(JSON.stringify([this.identity!.connectorId, messageId, index])).digest("hex")}`;
      const mime = item.mimeType == null ? "application/octet-stream" : text(item.mimeType, 256);
      const filename = item.filename == null ? "attachment" : text(item.filename, 1024);
      const kind = ["image", "video", "audio"].includes(item.type) ? item.type : "file";
      await this.event({ kind: "media_begin", attachmentId, messageId, sizeBytes: buffer.length, mime, filename });
      for (let offset = 0, seq = 0; offset < buffer.length; offset += SDK_WORKER_LIMITS.chunkBytes, seq++) {
        if (this.stopped) return;
        await this.event({ kind: "media_chunk", attachmentId, seq, data: buffer.subarray(offset, offset + SDK_WORKER_LIMITS.chunkBytes).toString("base64") });
      }
      await this.event({ kind: "media_end", attachmentId });
      attachments.push({ attachmentId, kind, platformRef: item.platformRef == null ? null : text(item.platformRef, 16_384), filename, mime, sizeBytes: buffer.length });
    }
    const rawName = message.displayName || message.senderName;
    const displayName = typeof rawName === "string" && rawName.trim() && !rawName.includes("\0") && Buffer.byteLength(rawName) <= 4096 ? rawName.trim() : undefined;
    const avatarUrl = this.publicAvatar(message.avatarUrl);
    if (!this.stopped) await this.event({ kind: "message", messageId, conversationId, conversationKind: message.isGroup === true ? "group" : "dm", senderId, text: content, attachments, replyContext: context, ...(displayName ? { displayName } : {}), ...(avatarUrl ? { avatarUrl } : {}) });
  }
  private publicAvatar(raw: unknown): string | undefined {
    if (typeof raw !== "string" || !raw.trim() || Buffer.byteLength(raw) > 4096 || raw.includes("\0")) return;
    try {
      const parsed = new URL(raw);
      if (!["https:", "http:"].includes(parsed.protocol) || parsed.username || parsed.password) return;
      if ([...parsed.searchParams.keys()].some(key => /token|secret|signature|credential|auth|access.?key|sign$/i.test(key))) return;
      let decoded = raw;
      for (let pass = 0; pass < 3; pass++) {
        if (this.secrets.some(secret => decoded.includes(secret))) return;
        const next = decodeURIComponent(decoded); if (next === decoded) break; decoded = next;
      }
      return parsed.href;
    } catch { return; }
  }

  private async download(item: Json, messageId: string): Promise<Buffer> {
    if (item.size != null && (!Number.isSafeInteger(item.size) || item.size < 0 || item.size > SDK_WORKER_LIMITS.attachmentBytes)) throw new Invalid("attachment_too_large");
    const platform = this.identity!.platform;
    let buffer: Buffer;
    if (platform === "feishu") buffer = item.type === "image" ? await this.adapter.downloadImage(text(item.platformRef), messageId, { signal: this.abort.signal }) : await this.adapter.downloadFile(messageId, text(item.platformRef), { signal: this.abort.signal });
    else if (platform === "wechat") buffer = await this.adapter.downloadFileByRef(text(item.platformRef, 16_384));
    else {
      let url = text(item.url, 16_384); if (url.startsWith("//")) url = `https:${url}`;
      const parsed = new URL(url);
      const suffixes = platform === "telegram" ? ["telegram.org", "telegram-cdn.org"] : ["qq.com", "qqbot.com", "qpic.cn", "gtimg.cn"];
      if (parsed.protocol !== "https:" || parsed.username || parsed.password || !suffixes.some(suffix => parsed.hostname === suffix || parsed.hostname.endsWith(`.${suffix}`))) throw new Invalid("untrusted_attachment_url");
      const response = await fetch(url, { signal: this.abort.signal, redirect: "error" });
      if (!response.ok || !response.body) throw new Invalid("attachment_download_failed");
      const chunks: Buffer[] = []; let size = 0;
      for await (const chunk of response.body) { const bytes = Buffer.from(chunk); size += bytes.length; if (size > SDK_WORKER_LIMITS.attachmentBytes) throw new Invalid("attachment_too_large"); chunks.push(bytes); }
      buffer = Buffer.concat(chunks, size);
    }
    if (!Buffer.isBuffer(buffer) || buffer.length > SDK_WORKER_LIMITS.attachmentBytes) throw new Invalid("attachment_too_large");
    return buffer;
  }

  stop(): Promise<void> {
    if (this.closing) return this.closing;
    this.phase = "stopping"; this.abort.abort(); this.resolveEffectsReady();
    this.closing = Promise.resolve().then(async () => {
      let failed = false;
      try { await this.adapter?.stop(); } catch { failed = true; }
      await Promise.allSettled([...this.tasks]);
      this.effects?.clear();
      this.dispatches.clear(); this.deliveries.clear(); this.secrets = [];
      this.media.clear(); this.reservedBytes = 0; this.reservedAttachments = 0;
      this.phase = "stopped";
      if (failed) await this.failure(new Invalid("platform_shutdown_failed"));
      await this.event({ kind: "stopped" });
    });
    return this.closing;
  }
}

/** 有界JSONL读写；断管、输入截断和信号都走同一个SDK回收入口。 */
export async function runBridgeSdkWorker(input: Readable, output: Writable, signals: NodeJS.Process = process): Promise<void> {
  let queued = 0;
  let writing: Promise<void> = Promise.resolve();
  const worker = new BridgeSdkWorker(async event => {
    const bytes = Buffer.from(`${JSON.stringify(event)}\n`);
    if (bytes.length > SDK_WORKER_LIMITS.lineBytes || queued + bytes.length > 1024 * 1024) throw new Invalid("output_capacity_exhausted");
    queued += bytes.length;
    const task = writing.then(() => new Promise<void>((resolve, reject) => output.write(bytes, error => error ? reject(error) : resolve())));
    writing = task.catch(() => {});
    try { await task; } finally { queued -= bytes.length; }
  });
  const shutdown = () => { void worker.stop().finally(() => input.destroy()).catch(() => {}); };
  signals.on("SIGTERM", shutdown); signals.on("SIGINT", shutdown);
  output.on("error", shutdown); input.on("error", shutdown);
  let pending = Buffer.alloc(0);
  try {
    for await (const chunk of input) {
      const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
      let start = 0;
      while (start < bytes.length) {
        const newline = bytes.indexOf(10, start);
        const end = newline < 0 ? bytes.length : newline;
        if (pending.length + end - start > SDK_WORKER_LIMITS.lineBytes) throw new Invalid("frame_too_large");
        pending = Buffer.concat([pending, bytes.subarray(start, end)]);
        if (newline < 0) break;
        const line = new TextDecoder("utf-8", { fatal: true }).decode(pending);
        pending = Buffer.alloc(0); start = newline + 1;
        if (!line.trim()) throw new Invalid("empty_frame");
        await worker.accept(JSON.parse(line));
        if (worker.stopped) break;
      }
      if (worker.stopped) break;
    }
    if (pending.length && !worker.stopped) throw new Invalid("truncated_frame");
  } catch (error) {
    if (!worker.stopped) throw error;
  } finally {
    await worker.stop(); await writing;
    signals.removeListener("SIGTERM", shutdown); signals.removeListener("SIGINT", shutdown);
    output.removeListener("error", shutdown); input.removeListener("error", shutdown);
  }
}
