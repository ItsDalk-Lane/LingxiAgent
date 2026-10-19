/**
 * wechat-login.js — 微信 iLink 扫码登录模块
 *
 * 独立于 adapter 生命周期，被 REST route 直接调用。
 * 参考 @tencent-weixin/openclaw-weixin v1.0.2 的 src/auth/login-qr.ts（MIT 协议）。
 */

import QRCode from "qrcode";

const BASE_URL = "https://ilinkai.weixin.qq.com";
const BOT_TYPE = "3";

/**
 * 构造 iLink 请求头（登录阶段无需 Authorization）
 */
function loginHeaders() {
  return {
    "iLink-App-ClientVersion": "1",
  };
}

async function loginJson(response: Response): Promise<Record<string, any>> {
  if (!response.body) throw new Error("登录响应为空");
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of response.body) {
    const bytes = Buffer.from(chunk); size += bytes.length;
    if (size > 64 * 1024) throw new Error("登录响应超过大小限制");
    chunks.push(bytes);
  }
  return JSON.parse(Buffer.concat(chunks, size).toString("utf8"));
}

/**
 * 获取微信扫码登录二维码
 * @returns {Promise<{ ok: boolean, qrcodeUrl?: string, qrcodeId?: string, error?: string }>}
 */
export async function getWechatQrcode(options: { signal?: AbortSignal } = {}) {
  try {
    const url = `${BASE_URL}/ilink/bot/get_bot_qrcode?bot_type=${BOT_TYPE}`;
    const res = await fetch(url, { headers: loginHeaders(), signal: options.signal ? AbortSignal.any([options.signal, AbortSignal.timeout(10_000)]) : AbortSignal.timeout(10_000), redirect: "error" });
    if (!res.ok) {
      return { ok: false, error: `HTTP ${res.status}` };
    }
    const data = await loginJson(res);
    if (!data.qrcode) {
      return { ok: false, error: "服务器未返回二维码" };
    }
    // qrcode_img_content 是要被编码成二维码的 URL 文本，不是图片
    // 用 qrcode 库转成 data URL（base64 PNG），前端直接 <img src> 显示
    const qrText = data.qrcode_img_content || data.qrcode;
    if (typeof qrText !== "string" || Buffer.byteLength(qrText) > 4096) throw new Error("二维码内容无效");
    const qrcodeDataUrl = await QRCode.toDataURL(qrText, { width: 280, margin: 2 });
    return {
      ok: true,
      qrcodeUrl: qrcodeDataUrl,
      qrcodeId: data.qrcode,
    };
  } catch (err) {
    return { ok: false, error: err.message };
  }
}

/**
 * 轮询微信扫码状态
 *
 * iLink 服务器会 hold 连接最多 35 秒（长轮询），
 * 所以前端不需要高频调用，每次请求本身就会等待。
 *
 * @param {string} qrcodeId - 从 getWechatQrcode 获取的 qrcode 值
 * @returns {Promise<{ status: string, botToken?: string, botId?: string, userId?: string, baseUrl?: string, error?: string }>}
 */
export async function pollWechatQrcodeStatus(qrcodeId, options: { signal?: AbortSignal } = {}) {
  if (!qrcodeId) {
    return { status: "error", error: "qrcodeId is required" };
  }

  try {
    const url = `${BASE_URL}/ilink/bot/get_qrcode_status?qrcode=${encodeURIComponent(qrcodeId)}`;
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 40_000);

    let res;
    try {
      res = await fetch(url, { headers: loginHeaders(), signal: options.signal ? AbortSignal.any([options.signal, controller.signal]) : controller.signal, redirect: "error" });
      clearTimeout(timer);
    } catch (err) {
      clearTimeout(timer);
      if (err.name === "AbortError") {
        return { status: "waiting" };
      }
      throw err;
    }

    if (!res.ok) {
      return { status: "error", error: `HTTP ${res.status}` };
    }

    const data = await loginJson(res);

    switch (data.status) {
      case "wait":
        return { status: "waiting" };
      case "scaned":
        return { status: "scanned" };
      case "confirmed":
        if (!data.bot_token || !data.ilink_bot_id) {
          return { status: "error", error: "登录成功但服务器未返回凭证" };
        }
        return {
          status: "confirmed",
          botToken: data.bot_token,
          botId: data.ilink_bot_id,
          userId: data.ilink_user_id,
          baseUrl: data.baseurl,
        };
      case "expired":
        return { status: "expired" };
      default:
        return { status: data.status || "waiting" };
    }
  } catch (err) {
    return { status: "error", error: err.message };
  }
}
