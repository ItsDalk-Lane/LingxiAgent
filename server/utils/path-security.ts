/**
 * path-security.js — 路径安全校验共享模块
 *
 * 敏感路径检查，供 upload.js 和 desk.js 共用。
 */
export { isSensitivePath, realPath } from "../../shared/path-security.ts";
