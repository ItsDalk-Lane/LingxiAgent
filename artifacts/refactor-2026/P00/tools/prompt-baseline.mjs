#!/usr/bin/env node
/**
 * P00-T06 提示词预算基线：在固定 fixture 下测量最终请求的来源分解（可静态复算部分）。
 * 组分：
 *   A. 系统提示词正文（golden zh/en——现有测试捕获的当前真实输出）
 *   B. 按需目录引导文本（core/engine.ts:185 的目录 intro）
 *   C. PTC 入口工具描述（lib/tools/ptc-tool.ts RUN_TOOLS_DESCRIPTION）
 *   D. 常驻工具面（4 工具名；schema 属 Pi SDK，运行时尺寸留待 P06 payload 捕获）
 *   E. 动态用户资料（人格/记忆/任务资料）——P06 用 model-call-payload 捕获补全
 * token 口径：lib/llm/estimate-text-tokens.ts（仓库唯一共享估算器，CJK 感知）；
 * 本基线为估算值，非精确 tokenizer；字节为 UTF-8 字节数。
 */
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import console from "node:console";
import { Buffer } from "node:buffer";
import { fileURLToPath } from "node:url";

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..");
const { estimateTextTokens } = await import(path.join(REPO, "lib", "llm", "estimate-text-tokens.ts"));
const { RUN_TOOLS_TOOL_NAME } = await import(path.join(REPO, "lib", "tools", "ptc-tool.ts"));

function measure(label, text) {
  const bytes = Buffer.byteLength(text, "utf-8");
  return { component: label, bytes, chars: text.length, estimated_tokens: estimateTextTokens(text) };
}

const goldenZh = fs.readFileSync(path.join(REPO, "tests", "fixtures", "system-prompt-golden-zh.txt"), "utf-8");
const goldenEn = fs.readFileSync(path.join(REPO, "tests", "fixtures", "system-prompt-golden-en.txt"), "utf-8");

// 目录 intro（与 core/engine.ts:185-186 同源文本；此处按运行时字符串重建）
const catalogIntroZh = "以下目录里的工具未随会话预载：用 mcp_search_tools 按关键词查找，mcp_describe_tool 查看参数，mcp_call 调用（内置工具可省略 server）。";
const catalogIntroEn = "The tools listed below are not preloaded: find them with mcp_search_tools, read parameters with mcp_describe_tool, and call them with mcp_call (server may be omitted for built-in tools).";

// PTC 入口描述：从生产模块导出的常量重建（import 上的 RUN_TOOLS_DESCRIPTION 未导出，按源文件解析）
const ptcSrc = fs.readFileSync(path.join(REPO, "lib", "tools", "ptc-tool.ts"), "utf-8");
const descMatch = ptcSrc.match(/const RUN_TOOLS_DESCRIPTION = \[([\s\S]*?)\]\.join\(" "\);/);
if (!descMatch) { console.error("RUN_TOOLS_DESCRIPTION 解析失败"); process.exit(1); }
const runToolsDescription = descMatch[1].split("\n").map((l) => l.trim().replace(/^"|",?$/g, "")).filter(Boolean).join(" ");

const components = [
  measure("A1 系统提示词正文（golden zh，当前真实输出）", goldenZh),
  measure("A2 系统提示词正文（golden en，当前真实输出）", goldenEn),
  measure("B1 按需目录引导（zh）", catalogIntroZh),
  measure("B2 按需目录引导（en）", catalogIntroEn),
  measure(`C PTC 入口工具 ${RUN_TOOLS_TOOL_NAME} 描述`, runToolsDescription),
];

const result = {
  schema_version: 1,
  stage_id: "P00",
  task_id: "P00-T06",
  captured_at: new Date().toISOString(),
  fixture: "tests/fixtures/system-prompt-golden-{zh,en}.txt + 生产模块常量（core/engine.ts:185, lib/tools/ptc-tool.ts）",
  token_metric: "estimateTextTokens（lib/llm/estimate-text-tokens.ts；CJK=1.1 token/字，其余 4 chars/token）——估算值，非精确 tokenizer",
  resident_tool_surface: ["read", "write", "edit", "exec_command"],
  deferred_catalog_policy: "其余全部按需（shared/tool-categories.ts ONDEMAND_CORE_TOOL_NAMES 推导）",
  components,
  not_measured_yet: [
    "常驻 4 工具的 JSON schema 字节（Pi SDK 运行时产物）——P06 用 payload 捕获",
    "动态用户资料（人格文件/记忆注入/任务资料）——P06 同一 fixture 下捕获最终 request",
    "消息历史与工具结果正文——属会话内容，不入常驻预算",
  ],
  budget_rule: "常驻平台开销（A+B+C+D）不得超过本基线；动态资料另列。不得删用户内容或关闭记忆凑数。",
};
fs.mkdirSync(path.join(REPO, "docs", "refactor-2026", "P00"), { recursive: true });
fs.writeFileSync(path.join(REPO, "docs", "refactor-2026", "P00", "PROMPT_BASELINE.json"), JSON.stringify(result, null, 2));
console.log(JSON.stringify({ written: "docs/refactor-2026/P00/PROMPT_BASELINE.json", components: components.map((c) => ({ c: c.component, bytes: c.bytes, est_tokens: c.estimated_tokens })) }, null, 2));
