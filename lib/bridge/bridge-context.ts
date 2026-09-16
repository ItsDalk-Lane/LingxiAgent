import { parseSessionKey } from "./session-key.ts";
import { interactionCapabilitiesForPlatform } from "./interaction-capabilities.ts";

export const BRIDGE_NOTIFY_PLATFORMS = ["wechat", "feishu", "dingtalk", "telegram", "qq"];

const PLATFORM_LABELS = {
  zh: {
    wechat: "微信",
    feishu: "飞书",
    dingtalk: "钉钉",
    telegram: "Telegram",
    qq: "QQ",
  },
  en: {
    wechat: "WeChat",
    feishu: "Feishu",
    dingtalk: "DingTalk",
    telegram: "Telegram",
    qq: "QQ",
  },
};

function localeKey(locale) {
  return String(locale || "").startsWith("zh") ? "zh" : "en";
}

export function bridgePlatformLabel(platform, locale = "zh") {
  const key = localeKey(locale);
  return PLATFORM_LABELS[key][platform] || platform || null;
}

export function normalizeBridgePlatforms(value) {
  const raw = Array.isArray(value) ? value : typeof value === "string" && value ? [value] : [];
  const bridgePlatforms = [];
  const invalidBridgePlatforms = [];
  for (const item of raw) {
    const platform = typeof item === "string" ? item.trim() : "";
    if (!platform) continue;
    if (!BRIDGE_NOTIFY_PLATFORMS.includes(platform)) {
      invalidBridgePlatforms.push(platform);
      continue;
    }
    if (!bridgePlatforms.includes(platform)) bridgePlatforms.push(platform);
  }
  return { bridgePlatforms, invalidBridgePlatforms };
}

export function buildBridgeContext(input: Record<string, any> = {}, locale = "zh") {
  const parsed = parseSessionKey(input.sessionKey || "");
  const platform = input.platform || parsed.platform;
  if (!BRIDGE_NOTIFY_PLATFORMS.includes(platform)) {
    return { isBridgeSession: false };
  }

  const chatType = input.chatType || parsed.chatType || "dm";
  const role = input.role || input.audience || (input.guest === true ? "guest" : "owner");
  const userId = input.userId || null;
  const chatId = input.chatId || parsed.chatId || null;
  const sessionKey = input.sessionKey || null;
  const agentId = input.agentId || parsed.agentId || null;
  const notificationHint = role === "owner" && chatType === "dm"
    ? {
        channels: ["bridge_owner"],
        bridgePlatforms: [platform],
        contextPolicy: "record_when_delivered",
      }
    : null;

  return {
    isBridgeSession: true,
    platform,
    platformLabel: bridgePlatformLabel(platform, locale),
    chatType,
    role,
    sessionKey,
    agentId,
    userId,
    chatId,
    notificationHint,
    // 派生字段：始终由平台声明表重建，不从持久化 meta / 缓存读取
    interactionCapabilities: interactionCapabilitiesForPlatform(platform),
  };
}

export function buildBridgePromptLine(context, locale = "zh") {
  if (!context?.isBridgeSession || !context.platform) return "";
  const label = bridgePlatformLabel(context.platform, locale);
  if (!label) return "";
  const zh = localeKey(locale) === "zh";
  const base = zh
    ? `当前用户正通过${label}与你对话，仅在需要理解当前平台或“这里”等指代时参考。`
    : `The user is currently talking with you through ${label}; use this only when interpreting the current platform or references like "here."`;
  const confirmation = buildTextCommandConfirmationGuidance(context, label, zh);
  if (!confirmation) return base;
  return zh ? `${base}${confirmation}` : `${base} ${confirmation}`;
}

/**
 * 文本指令确认指引（#1619）：按平台声明的 interactionCapabilities 分叉。
 * 使用文字确认的渠道不提供灵犀桌面的确认卡片；引导用户回复 /apply 等指令。
 * 这只描述灵犀操作的确认方式，不限制渠道的媒体或其他交互能力。
 */
function buildTextCommandConfirmationGuidance(context, label, zh) {
  if (context?.interactionCapabilities?.confirmationMode !== "text_command") return "";
  if (zh) {
    return `${label}中的灵犀操作用文字指令确认：`
      + "回复 /apply 创建最新建议中的自动任务，/apply <建议ID> 创建指定建议中的任务；"
      + "引导用户回复指令，不要让用户点击桌面确认卡片。";
  }
  return `Confirm Lingxi actions in ${label} by text command: `
    + "/apply creates the latest suggested task; /apply <id> creates the specified one. "
    + "Guide users to reply instead of clicking desktop confirmation cards.";
}

export function appendBridgePromptLine(prompt, context, locale = "zh") {
  const line = buildBridgePromptLine(context, locale);
  if (!line) return prompt || "";
  const base = prompt || "";
  if (base.includes(line)) return base;
  return `${base}\n\n${line}`;
}

export function bridgeContextIndexMeta(context, meta = {}) {
  if (!context?.isBridgeSession) return meta || null;
  return {
    ...(meta || {}),
    platform: context.platform,
    chatType: context.chatType,
    role: context.role,
    ...(context.userId ? { userId: context.userId } : {}),
    ...(context.chatId ? { chatId: context.chatId } : {}),
  };
}
