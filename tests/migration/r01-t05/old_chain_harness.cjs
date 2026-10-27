/**
 * R01-T05 旧链对照 harness（只读复用生产 desktop/src/office-pdf-helper.cjs）。
 *
 * 运行：node_modules/.bin/electron tests/migration/r01-t05/old_chain_harness.cjs \
 *         --hana-office-html-to-pdf <job.json>
 *
 * 隔离措施（红线：不得写用户目录、输出全部 /tmp、防后台外联）：
 *  - userData/sessionData/cache/logs/crashDumps 全部重定向到 R01_T05_OLD_TMP（默认 /tmp/r01t05/old-tmp）；
 *  - 全部网络走 loopback-only 代理（默认 127.0.0.1:18382，T04 修补版 proxy.mjs，
 *    非 loopback 一律 403），proxy-bypass-list=<-loopback> 关闭隐式绕过；
 *  - 本文件不修改生产代码，仅以生产同款 flag 调用 helper。
 */
"use strict";

const path = require("path");
const { app } = require("electron");

const TMP = process.env.R01_T05_OLD_TMP || "/tmp/r01t05/old-tmp";
const PROXY = process.env.R01_T05_PROXY || "127.0.0.1:18382";

for (const [key, dir] of [
  ["userData", "userData"],
  ["sessionData", "sessionData"],
  ["cache", "cache"],
  ["logs", "logs"],
  ["crashDumps", "crashDumps"],
]) {
  try { app.setPath(key, path.join(TMP, dir)); } catch {}
}
app.commandLine.appendSwitch("proxy-server", `http://${PROXY}`);
app.commandLine.appendSwitch("proxy-bypass-list", "<-loopback>");
app.commandLine.appendSwitch("disable-background-networking");
app.commandLine.appendSwitch("disable-component-update");
app.commandLine.appendSwitch("disable-sync");
app.commandLine.appendSwitch("metrics-recording-only");

const helper = require("../../../desktop/src/office-pdf-helper.cjs");

if (!helper.isOfficePdfHelperInvocation(process.argv)) {
  console.error("[r01-t05-old-harness] missing --hana-office-html-to-pdf flag");
  process.exit(2);
}

helper.runOfficePdfHelperFromArgv(process.argv).catch((err) => {
  console.error("[r01-t05-old-harness] FAILED:", err?.stack || err?.message || err);
  process.exitCode = 1;
  try { app.exit(1); } catch {}
});
