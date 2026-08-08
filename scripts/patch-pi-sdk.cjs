/**
 * patch-pi-sdk.cjs — Pi SDK 只读验证
 *
 * 历史上这个脚本会在 postinstall 阶段修改
 * node_modules/@mariozechner/pi-coding-agent/dist/core/sdk.js，
 * 为 Hana 的 session-scoped sandbox tools 打通 baseToolsOverride。
 *
 * Pi SDK 0.68+ 已把 createAgentSession({ tools }) 改成工具名 allowlist，
 * Hana 现在通过 lib/pi-sdk 适配层把本地 Tool[] 转为 customTools + names。
 * 因此这个脚本只验证版本、SDK 结构和生产 import 边界，不再写 node_modules。
 *
 * 文件名（patch-pi-sdk）保留是为了不动 package.json 的 postinstall 钩子，
 * 避免触发 npm install cache 重算。实际职责已是只读验证（log 前缀 verify-pi-sdk）。
 */

const fs = require("fs");
const path = require("path");

const root = path.join(__dirname, "..");
const sdkRoot = path.join(root, "node_modules", "@earendil-works", "pi-coding-agent");
const piAiRoot = path.join(root, "node_modules", "@earendil-works", "pi-ai");
const verifiedVersions = new Set(["0.80.3", "0.83.0", "0.84.1"]);
const verifiedPiAiVersions = new Set(["0.80.3", "0.83.0", "0.84.1"]);

function fail(message) {
  console.error(`[verify-pi-sdk] ${message}`);
  process.exit(1);
}

function readJson(file) {
  return JSON.parse(fs.readFileSync(file, "utf8"));
}

if (!fs.existsSync(sdkRoot)) {
  console.log("[verify-pi-sdk] SDK not installed, skipping");
  process.exit(0);
}

const pkg = readJson(path.join(sdkRoot, "package.json"));
if (!verifiedVersions.has(pkg.version)) {
  fail(`SDK version ${pkg.version} is not verified. Verified versions: ${[...verifiedVersions].join(", ")}`);
}

if (!fs.existsSync(piAiRoot)) {
  fail("@earendil-works/pi-ai is not installed");
}
const piAiPkg = readJson(path.join(piAiRoot, "package.json"));
if (!verifiedPiAiVersions.has(piAiPkg.version)) {
  fail(`pi-ai version ${piAiPkg.version} is not verified. Verified versions: ${[...verifiedPiAiVersions].join(", ")}`);
}

const sdkIndex = fs.readFileSync(path.join(sdkRoot, "dist", "index.js"), "utf8");
const expectedExportMarkers = [
  "createAgentSession",
  "createReadTool",
  "createWriteTool",
  "createEditTool",
  "createBashTool",
  "createGrepTool",
  "createFindTool",
  "createLsTool",
  "parseSessionEntries",
  "buildSessionContext",
];

for (const marker of expectedExportMarkers) {
  if (!sdkIndex.includes(marker)) {
    fail(`expected SDK export marker not found: ${marker}`);
  }
}

// 生产 import 边界：lib/pi-sdk 是所有 pi SDK production imports 的唯一边界。
// 扫描 core/server/lib/hub，禁止越界直接 import @earendil-works/*（lib/pi-sdk 除外）。
// pi-ai / pi-coding-agent / pi-agent-core 三个包一视同仁——任一直接 import 都算泄漏。
// （此前 regex 漏列 pi-agent-core；当前虽无生产泄漏——lib/pi-sdk/index.ts 从它
//  re-export runAgentLoop 等属合法边界内消费——补齐以闭合静态覆盖。）
const scanDirs = ["core", "server", "lib", "hub"].map(d => path.join(root, d));
const adapterDir = path.join(root, "lib", "pi-sdk");
const importPattern = /(?:from\s+["']@(?:mariozechner|earendil-works)\/(?:pi-ai|pi-coding-agent|pi-agent-core)|import\s*\(\s*["']@(?:mariozechner|earendil-works)\/(?:pi-ai|pi-coding-agent|pi-agent-core)|require\s*\(\s*["']@(?:mariozechner|earendil-works)\/(?:pi-ai|pi-coding-agent|pi-agent-core))/;
const leaks = [];

function scanDir(dir) {
  if (!fs.existsSync(dir)) return;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (full === adapterDir || entry.name === "node_modules") continue;
      scanDir(full);
    } else if (/\.(js|mjs|cjs|ts)$/.test(entry.name)) {
      const content = fs.readFileSync(full, "utf8");
      if (importPattern.test(content)) {
        leaks.push(path.relative(root, full));
      }
    }
  }
}

for (const dir of scanDirs) scanDir(dir);

if (leaks.length > 0) {
  fail(`production files bypass lib/pi-sdk: ${leaks.join(", ")}`);
}

console.log("[verify-pi-sdk] all checks passed");
