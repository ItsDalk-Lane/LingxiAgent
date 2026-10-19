/** 单次凭据验证和微信扫码；不创建适配器、不接收消息、不打开轮询连接。 */
import { randomBytes } from "node:crypto";
import type { Readable, Writable } from "node:stream";
import { SDK_WORKER_LIMITS, sdkCredentials } from "./sdk-worker.ts";
import type { Platform } from "./sdk-worker.ts";
import { telegramBotOptions } from "../net/outbound-proxy.ts";
import { resolveFeishuDomain } from "./feishu-adapter.ts";
import { normalizeDingTalkBridgeCredentials } from "./dingtalk-contract.ts";
import { requestDingTalkAccessToken } from "./dingtalk-api.ts";
import { getWechatQrcode, pollWechatQrcodeStatus } from "./wechat-login.ts";
import type { BridgeDiagnosticInfo, BridgeDiagnosticResult, BridgeQrcodeStatus } from "../../contracts/generated/ts/lingxi-protocol.ts";

type Json = Record<string, any>;
function object(value: unknown): Json { if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("object_required"); return value as Json; }
function keys(value: Json, allowed: string[]) { if (Object.keys(value).some(key => !allowed.includes(key))) throw new Error("unexpected_field"); }
function text(value: unknown, max = 1024): string { if (typeof value !== "string" || !value.trim() || value.includes("\0") || Buffer.byteLength(value) > max) throw new Error("invalid_text"); return value; }
function optional(value: unknown, max = 1024): string | undefined { return value == null || value === "" ? undefined : text(value, max); }
async function json(url: string, options: RequestInit = {}, signal?: AbortSignal): Promise<Json> {
  const response = await fetch(url, { ...options, signal: signal ? AbortSignal.any([signal, AbortSignal.timeout(10_000)]) : AbortSignal.timeout(10_000), redirect: "error" });
  if (!response.ok || !response.body) throw new Error("platform_http_failed");
  let size = 0; const chunks: Buffer[] = [];
  for await (const chunk of response.body) { const bytes = Buffer.from(chunk); size += bytes.length; if (size > 64 * 1024) throw new Error("platform_response_too_large"); chunks.push(bytes); }
  return object(JSON.parse(Buffer.concat(chunks, size).toString("utf8")));
}

async function verify(platform: Platform, input: unknown, signal?: AbortSignal): Promise<BridgeDiagnosticResult> {
  const raw = object(input);
  const config = sdkCredentials(platform, { ...raw, agentId: raw.agentId ?? "diagnostic" });
  const info: BridgeDiagnosticInfo = { credentialOk: true };
  if (platform === "telegram") {
    const TelegramBot = (await import("node-telegram-bot-api")).default;
    const bot = new TelegramBot(config.token, telegramBotOptions({ polling: false }));
    try {
      const me = await bot.getMe();
      if (!Number.isSafeInteger(me.id) || me.id <= 0) throw new Error("invalid_platform_identity");
      info.username = optional(me.username); info.name = optional(me.first_name);
    } finally { bot.removeAllListeners(); await bot.stopPolling(); }
  } else if (platform === "feishu") {
    const domain = resolveFeishuDomain(config.region);
    const result = await json(`${domain.domain}/open-apis/auth/v3/tenant_access_token/internal`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ app_id: config.appId, app_secret: config.appSecret }) }, signal);
    if (result.code !== 0 || !result.tenant_access_token) throw new Error("platform_credentials_rejected");
  } else if (platform === "dingtalk") {
    const contract = normalizeDingTalkBridgeCredentials(config);
    await requestDingTalkAccessToken(contract, async request => fetch(request.url, { ...request.init, signal: signal ? AbortSignal.any([signal, AbortSignal.timeout(10_000)]) : AbortSignal.timeout(10_000), redirect: "error" }));
    info.authMode = contract.authMode;
  } else if (platform === "qq") {
    const token = await json("https://bots.qq.com/app/getAppAccessToken", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ appId: config.appID, clientSecret: config.appSecret }) }, signal);
    text(token.access_token, 16_384);
    const me = await json("https://api.sgroup.qq.com/users/@me", { headers: { Authorization: `QQBot ${token.access_token}` } }, signal);
    text(me.id); info.username = optional(me.username); info.name = optional(me.username);
  } else {
    const uin = Buffer.from(String(randomBytes(4).readUInt32BE(0))).toString("base64");
    const result = await json("https://ilinkai.weixin.qq.com/ilink/bot/getconfig", { method: "POST", headers: { "Content-Type": "application/json", AuthorizationType: "ilink_bot_token", Authorization: `Bearer ${config.botToken}`, "X-WECHAT-UIN": uin }, body: JSON.stringify({ base_info: { channel_version: "1.0.0" } }) }, signal);
    if (result.ret !== 0) throw new Error("platform_credentials_rejected");
  }
  return { kind: "diagnostic_result", ok: true, info };
}

export async function runSdkDiagnostic(input: unknown, signal?: AbortSignal): Promise<BridgeDiagnosticResult> {
  try {
    const command = object(input);
    if (command.kind === "verify") { keys(command, ["kind", "platform", "config"]); return await verify(command.platform, command.config, signal); }
    if (command.kind === "wechat_qrcode") {
      keys(command, ["kind"]);
      const result = await getWechatQrcode({ signal });
      if (!result.ok) throw new Error("login_failed");
      const url = text(result.qrcodeUrl, 200_000);
      if (!url.startsWith("data:image/png;base64,")) throw new Error("invalid_qrcode");
      return { kind: "diagnostic_result", ok: true, qrcodeUrl: url, qrcodeId: text(result.qrcodeId, 4096) };
    }
    if (command.kind === "wechat_qrcode_status") {
      keys(command, ["kind", "qrcodeId"]);
      const result = await pollWechatQrcodeStatus(text(command.qrcodeId, 4096), { signal });
      if (!["waiting", "scanned", "confirmed", "expired"].includes(result.status)) throw new Error("login_failed");
      if (result.status !== "confirmed") return { kind: "diagnostic_result", ok: true, status: result.status as BridgeQrcodeStatus };
      if (result.baseUrl && new URL(result.baseUrl).origin !== "https://ilinkai.weixin.qq.com") throw new Error("untrusted_platform_endpoint");
      return { kind: "diagnostic_result", ok: true, status: "confirmed", botToken: text(result.botToken, 16_384), botId: text(result.botId), userId: optional(result.userId), baseUrl: optional(result.baseUrl) };
    }
    throw new Error("unknown_diagnostic_operation");
  } catch {
    return { kind: "diagnostic_result", ok: false, error: "diagnostic_request_failed" };
  }
}

/** 只读首个完整有界帧，立刻退出输入循环；从不建立持久 SDK 连接。 */
export async function runBridgeSdkDiagnostic(input: Readable, output: Writable, signal?: AbortSignal): Promise<void> {
  let result: BridgeDiagnosticResult = { kind: "diagnostic_result", ok: false, error: "diagnostic_request_invalid" };
  try {
    let pending = Buffer.alloc(0);
    for await (const chunk of input) {
      const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
      const end = bytes.indexOf(10);
      if (pending.length + (end < 0 ? bytes.length : end) > SDK_WORKER_LIMITS.lineBytes) throw new Error("frame_too_large");
      pending = Buffer.concat([pending, end < 0 ? bytes : bytes.subarray(0, end)]);
      if (end < 0) continue;
      if (end !== bytes.length - 1) throw new Error("multiple_diagnostic_frames");
      const line = new TextDecoder("utf8", { fatal: true }).decode(pending);
      result = await runSdkDiagnostic(JSON.parse(line), signal);
      break;
    }
  } catch { /* 协议错误只返回固定原因，不回显输入或凭据。 */ }
  const bytes = Buffer.from(`${JSON.stringify(result)}\n`);
  if (bytes.length > SDK_WORKER_LIMITS.lineBytes) throw new Error("diagnostic_result_too_large");
  await new Promise<void>((resolve, reject) => output.write(bytes, error => error ? reject(error) : resolve()));
}
