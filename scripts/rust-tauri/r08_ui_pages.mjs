#!/usr/bin/env node
/**
 * R08 UI 5 叶逐断言客户端证据生产者。
 *
 * 用法：
 *   node scripts/rust-tauri/r08_ui_pages.mjs --out <evidence目录> [--no-live]
 *
 * 做什么：
 *   1. 跑选定 vitest 文件（verbose + json 双 reporter），把 case → 测试名 映射
 *      解析成逐 case 的 pass/fail；
 *   2. 跑两条真实的 Electron live 套件（快捷窗口 / 会话地图，真 Vite + 真 Electron
 *      + 真 lingxi-service 子进程），把 case → 真实界面 check 映射解析成 pass/fail；
 *   3. 写 <out>/ui-cases.json（schema lingxi.leaf-case-results.v1，逐 case ok）与
 *      <out>/output.log、<out>/result.json；
 *   4. 任何映射缺失 / 测试失败 / 测试跳过 / live 不 PASS → exit 非 0。脚本只生产
 *      证据，不判叶子完成，也不修改任何正式门文件。
 *
 * 环境约束：
 *   - 本机 shell 可能带着 ELECTRON_RUN_AS_NODE=1（在 Electron 宿主内执行时），
 *     live 子进程必须清掉它，否则 Electron 会以纯 Node 运行、拿不到 app。
 *   - Rust 服务二进制固定用 rust/target/debug/lingxi-service（由外层构建；缺失即失败，
 *     不静默跳过 live 证据）。
 */

import { spawn } from 'node:child_process';
import { access, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';

const ROOT = path.resolve(import.meta.dirname, '..', '..');
const SERVICE_BIN = path.join(ROOT, 'rust', 'target', 'debug', 'lingxi-service');
const NPX = process.platform === 'win32' ? 'npx.cmd' : 'npx';
const PYTHON = process.platform === 'win32' ? 'python' : 'python3';

const args = process.argv.slice(2);
function argValue(name) {
  const index = args.indexOf(name);
  return index >= 0 ? args[index + 1] : null;
}
if (!args.includes('--out')) {
  console.error('usage: node scripts/rust-tauri/r08_ui_pages.mjs --out <evidence-dir> [--no-live]');
  process.exit(2);
}
const OUT = path.resolve(argValue('--out'));
const NO_LIVE = args.includes('--no-live');

const NEW_UI = [
  'desktop/src/react/__tests__/r08/r08-ui-settings-container.test.tsx',
  'desktop/src/react/__tests__/r08/r08-ui-settings-mcp.test.tsx',
  'desktop/src/react/__tests__/r08/r08-ui-settings-security.test.tsx',
  'desktop/src/react/__tests__/r08/r08-ui-settings-guards.test.ts',
  'desktop/src/react/__tests__/r08/r08-ui-chat-page.test.tsx',
];
const EXISTING_UI = [
  'desktop/src/react/__tests__/components/ModelSelector.test.tsx',
  'desktop/src/react/__tests__/components/thinking-level-button.test.tsx',
  'desktop/src/react/__tests__/components/InputArea.media-send.test.tsx',
  'desktop/src/react/__tests__/components/InputArea.paste-and-slash.test.tsx',
  'desktop/src/react/__tests__/components/slash-commands-ws.test.ts',
  'desktop/src/react/__tests__/services/ws-message-handler.test.ts',
  'desktop/src/react/__tests__/services/stream-resume.test.ts',
  'desktop/src/react/__tests__/components/SideChatPanel.test.tsx',
  'desktop/src/react/__tests__/components/AttachedFilesBar.media.test.tsx',
  'desktop/src/react/__tests__/components/RuntimeInfoCapsule.test.tsx',
];
// composer-send.transaction.test.ts 在本工作树有与本证据无关的基线红用例
// （"Rust wire 只有握手时拒绝旧聊天帧且不清空输入"，属并行批次改动面），
// 因此只用 -t 精确选取本叶需要的两条并如实记录。
const COMPOSER = ['desktop/src/react/__tests__/components/composer-send.transaction.test.ts'];
const COMPOSER_PATTERN = 'P07：全空白正文|S04：ws 不存在';

/** 逐断言 case 名单：case → 证据（vitest 测试或 live check）。叶内不复用。 */
const CASES = [
  // ── chat 页叶（6 条断言，0-based a0..a5）──
  // a0 打开聊天页显示欢迎页/消息流与输入框
  vitest('ui::chat.a0.welcome-input-exclusive-projection', 'chat', 0,
    'desktop/src/react/__tests__/r08/r08-ui-chat-page.test.tsx',
    'R08 聊天页：欢迎态/消息流/输入区（断言 1） 无会话显示欢迎页与输入框；选中当前会话后欢迎页隐藏、消息区与输入区并存'),
  vitest('ui::chat.a0.mobile-disconnected-no-thinking-get', 'chat', 0,
    'desktop/src/react/__tests__/r08/r08-ui-chat-page.test.tsx',
    'R08 聊天页：欢迎态/消息流/输入区（断言 1） mobile 或服务未连接时不请求该会话思考档位 GET'),
  vitest('ui::chat.a0.regional-failure-boundary', 'chat', 0,
    'desktop/src/react/__tests__/r08/r08-ui-chat-page.test.tsx',
    'R08 聊天页：欢迎态/消息流/输入区（断言 1） 区域渲染失败由 RegionalErrorBoundary 捕获并显示重试，不冒充消息成功'),
  // a1 输入提交普通 prompt 或 slash
  vitest('ui::chat.a1.optimistic-user-message-client-id', 'chat', 1,
    'desktop/src/react/__tests__/components/InputArea.paste-and-slash.test.tsx',
    'InputArea paste and slash menu behavior adds an optimistic user message with a clientMessageId when sending a prompt'),
  vitest('ui::chat.a1.slash-sent-via-ws', 'chat', 1,
    'desktop/src/react/__tests__/components/slash-commands-ws.test.ts',
    'executeSlashViaWs carries the agent that owns this input box so the server never has to guess'),
  vitest('ui::chat.a1.slash-result-inline-notice', 'chat', 1,
    'desktop/src/react/__tests__/services/ws-message-handler.test.ts',
    'ws-message-handler turn_end side effects shows focused slash_result through the inline notice bridge'),
  vitest('ui::chat.a1.blank-input-blocked', 'chat', 1,
    'desktop/src/react/__tests__/components/composer-send.transaction.test.ts',
    'composer-send 显式结果与快照（S01–S15） P07：全空白正文（无其他内容维度）不发，且不做破坏性清空'),
  vitest('ui::chat.a1.ws-unavailable-explicit-result', 'chat', 1,
    'desktop/src/react/__tests__/components/composer-send.transaction.test.ts',
    'composer-send 显式结果与快照（S01–S15） S04：ws 不存在 / CONNECTING / CLOSING / CLOSED，明确结果、不清输入、不发消息'),
  // a2 中止、插话、运行中输入入队及恢复流
  vitest('ui::chat.a2.streaming-send-queues-then-interjects', 'chat', 2,
    'desktop/src/react/__tests__/components/InputArea.media-send.test.tsx',
    'InputArea media send interjects streaming attachment sends with the same message envelope as prompt sends'),
  vitest('ui::chat.a2.stop-binds-current-stream', 'chat', 2,
    'desktop/src/react/__tests__/components/InputArea.media-send.test.tsx',
    'InputArea media send binds Stop to the focused session current stream identity (#2078)'),
  vitest('ui::chat.a2.idle-vs-running-control', 'chat', 2,
    'desktop/src/react/__tests__/r08/r08-ui-chat-page.test.tsx',
    'R08 聊天页：运行状态控件（断言 3） 运行信息控件随真实运行态切换：无运行任务只有发送，会话在跑显示中止'),
  vitest('ui::chat.a2.resume-keeps-streaming-state', 'chat', 2,
    'desktop/src/react/__tests__/services/stream-resume.test.ts',
    'stream-resume keeps the session marked streaming when resume replay is empty but runtime says it is still running'),
  // a3 侧边对话、附件/预览、运行信息胶囊
  vitest('ui::chat.a3.side-chat-transcript-composer', 'chat', 3,
    'desktop/src/react/__tests__/components/SideChatPanel.test.tsx',
    'SideChatPanel renders the side session transcript and its own composer'),
  vitest('ui::chat.a3.side-chat-closed-empty', 'chat', 3,
    'desktop/src/react/__tests__/components/SideChatPanel.test.tsx',
    'SideChatPanel renders nothing while the panel is closed'),
  vitest('ui::chat.a3.attachments-preview', 'chat', 3,
    'desktop/src/react/__tests__/components/AttachedFilesBar.media.test.tsx',
    'AttachedFilesBar media chips renders image attachments with rounded thumbnail previews'),
  vitest('ui::chat.a3.runtime-capsule-pill', 'chat', 3,
    'desktop/src/react/__tests__/components/RuntimeInfoCapsule.test.tsx',
    'RuntimeInfoCapsule renders collapsed as a pill outside the document flow of the transcript'),
  // a4 主输入工具栏选择聊天模型
  vitest('ui::chat.a4.existing-session-model-switch', 'chat', 4,
    'desktop/src/react/__tests__/components/ModelSelector.test.tsx',
    'ModelSelector 显式 sessionPath 时按该会话切换，并把结果回调给调用方'),
  vitest('ui::chat.a4.new-session-default-model-set', 'chat', 4,
    'desktop/src/react/__tests__/components/ModelSelector.test.tsx',
    'ModelSelector applies the selected model thinking default while preparing a new session'),
  vitest('ui::chat.a4.same-model-zero-request', 'chat', 4,
    'desktop/src/react/__tests__/r08/r08-ui-chat-page.test.tsx',
    'R08 聊天页：模型切换空态（断言 5） 已建会话选择相同且可用模型直接返回、不发 switch 请求；无可选模型不产生有效切换'),
  vitest('ui::chat.a4.switch-failure-explicit-toast', 'chat', 4,
    'desktop/src/react/__tests__/components/ModelSelector.test.tsx',
    'ModelSelector maps a server streaming-switch rejection to the same explicit warning'),
  // a5 主输入工具栏选择思考档位
  vitest('ui::chat.a5.thinking-session-save', 'chat', 5,
    'desktop/src/react/__tests__/components/thinking-level-button.test.tsx',
    'ThinkingLevelButton saves thinking changes to the current session when a session is active'),
  vitest('ui::chat.a5.thinking-pending-default-only', 'chat', 5,
    'desktop/src/react/__tests__/components/thinking-level-button.test.tsx',
    'ThinkingLevelButton saves pending new-session thinking changes as the model default draft'),
  vitest('ui::chat.a5.thinking-save-failure-console-only', 'chat', 5,
    'desktop/src/react/__tests__/r08/r08-ui-chat-page.test.tsx',
    'R08 聊天页：思考档位空态/错误态（断言 6） 未建立会话时只改默认档位（请求不带 sessionPath）；保存失败仅 console.error，不宣称已持久化'),

  // ── map 页叶（7 条断言，0-based a0..a6；全部由真 Electron live 证据）──
  live('ui::map.a0.loads-real-layout-and-turn-cards', 'map', 0, 'session-map', 'production-map-loads-real-layout-and-turn-cards'),
  live('ui::map.a0.other-service-empty-map-zero-mutation', 'map', 0, 'session-map', 'actual-other-service-empty-map-clears-old-owner-with-zero-put'),
  live('ui::map.a0.read-failure-explicit-warning', 'map', 0, 'session-map', 'actual-read-failure-explicit-warning-without-success-or-disk-rewrite'),
  live('ui::map.a1.workspace-selector-no-cwd-zero-files-read', 'map', 1, 'session-map', 'workspace-selector-real-focus-with-no-cwd-zero-files-read'),
  live('ui::map.a1.card-selects-real-session-same-session-zero-fetch', 'map', 1, 'session-map', 'card-selects-real-session-files-models-and-same-session-zero-fetch'),
  live('ui::map.a1.same-revision-cache-zero-history-changed-revision-refresh', 'map', 1, 'session-map', 'same-revision-cache-zero-history-and-changed-revision-real-history-refresh'),
  live('ui::map.a1.archive-refusal-retains-session-selected-card', 'map', 1, 'session-map', 'archive-refusal-retains-current-session-but-selected-card-already-changes'),
  live('ui::map.a2.zoom-locate-only-change-real-camera', 'map', 2, 'session-map', 'zoom-locate-only-change-real-camera'),
  live('ui::map.a2.invalid-coordinates-zero-put', 'map', 2, 'session-map', 'invalid-local-coordinates-send-zero-put'),
  live('ui::map.a3.pointer-drag-persists-card-position', 'map', 3, 'session-map', 'real-pointer-drag-persists-card-position'),
  live('ui::map.a3.drag-write-failure-keeps-local-disk-unchanged', 'map', 3, 'session-map', 'real-write-failure-keeps-local-drag-but-disk-unchanged'),
  live('ui::map.a4.collapse-reload-and-external-reveal-replaces-list', 'map', 4, 'session-map', 'collapse-reload-and-external-current-reveal-replace-list'),
  live('ui::map.a4.collapse-write-failure-keeps-local-old-disk', 'map', 4, 'session-map', 'real-collapse-write-failure-keeps-local-collapse-and-old-disk-until-reload'),
  live('ui::map.a5.reset-replaces-all-positions-on-disk', 'map', 5, 'session-map', 'reset-replaces-all-positions-on-disk'),
  live('ui::map.a5.reset-write-failure-keeps-local-old-disk', 'map', 5, 'session-map', 'real-reset-write-failure-keeps-local-reset-but-old-disk'),
  live('ui::map.a6.empty-whitespace-zero-send-continue-no-fork', 'map', 6, 'session-map', 'empty-whitespace-zero-send-and-continue-real-prompt-without-fork-or-switch'),
  live('ui::map.a6.fork-failure-missing-anchor-preserves-draft-then-branch', 'map', 6, 'session-map', 'real-fork-IO-error-and-missing-anchor-preserve-draft-then-actual-branch-prompt-focus'),
  live('ui::map.a6.running-busy-queues-input-zero-extra-prompt', 'map', 6, 'session-map', 'actual-running-provider-busy-queues-input-with-zero-extra-prompt'),
  live('ui::map.a6.paused-unknown-delivery-preserves-draft', 'map', 6, 'session-map', 'real-paused-service-unknown-delivery-preserves-draft-and-zero-duplicate-submit'),

  // ── MCP 设置页叶（7 条断言，0-based a0..a6）──
  vitest('ui::mcp-settings.a0.container-cold-start-mcp-page', 'mcp-settings', 0,
    'desktop/src/react/__tests__/r08/r08-ui-settings-container.test.tsx',
    'R08 设置容器初始化（MCP 设置页叶 & 安全设置页叶共用断言 1） MCP 设置页冷启动：显示 loading mask，初始化完成后移除并真实渲染 MCP 页'),
  vitest('ui::mcp-settings.a0.cached-old-data-background-refresh', 'mcp-settings', 0,
    'desktop/src/react/__tests__/r08/r08-ui-settings-container.test.tsx',
    'R08 设置容器初始化（MCP 设置页叶 & 安全设置页叶共用断言 1） 已有缓存直接显示旧数据并在后台刷新，不阻塞 ready、不显示全屏 mask'),
  vitest('ui::mcp-settings.a0.provider-summary-nonblocking', 'mcp-settings', 0,
    'desktop/src/react/__tests__/r08/r08-ui-settings-container.test.tsx',
    'R08 设置容器初始化（MCP 设置页叶 & 安全设置页叶共用断言 1） 供应商摘要失败不阻塞 ready，也不冒充空列表成功'),
  vitest('ui::mcp-settings.a1.connector-list-status-tools', 'mcp-settings', 1,
    'desktop/src/react/__tests__/r08/r08-ui-settings-mcp.test.tsx',
    'R08 MCP 设置页：连接器状态与工具权限（断言 2） 读取真实连接器列表并逐条显示状态、工具数与助手权限；无连接器时显示添加入口'),
  vitest('ui::mcp-settings.a1.read-failure-keeps-old-state', 'mcp-settings', 1,
    'desktop/src/react/__tests__/r08/r08-ui-settings-mcp.test.tsx',
    'R08 MCP 设置页：连接器状态与工具权限（断言 2） 连接器读取失败显示显式错误并保留旧状态，不把失败冒充成空列表'),
  vitest('ui::mcp-settings.a2.global-toggle-save-reread', 'mcp-settings', 2,
    'desktop/src/react/__tests__/r08/r08-ui-settings-mcp.test.tsx',
    'R08 MCP 设置页：全局启停与延迟（断言 3） 总开关保存后按助手重读状态并显示真实成功提示'),
  vitest('ui::mcp-settings.a2.global-toggle-failure-restores', 'mcp-settings', 2,
    'desktop/src/react/__tests__/r08/r08-ui-settings-mcp.test.tsx',
    'R08 MCP 设置页：全局启停与延迟（断言 3） 总开关保存失败恢复旧配置并显示错误，不写成功状态'),
  vitest('ui::mcp-settings.a2.defer-threshold-save', 'mcp-settings', 2,
    'desktop/src/react/__tests__/r08/r08-ui-settings-mcp.test.tsx',
    'R08 MCP 设置页：全局启停与延迟（断言 3） 延迟加载开关与阈值保存到全局设置，并在保存后重读状态'),
  vitest('ui::mcp-settings.a2.no-agent-global-state-only', 'mcp-settings', 2,
    'desktop/src/react/__tests__/r08/r08-ui-settings-mcp.test.tsx',
    'R08 MCP 设置页：全局启停与延迟（断言 3） 无 agentId 时读取全局状态（不带 agentId 查询），不伪造助手权限'),
  vitest('ui::mcp-settings.a3.add-connector-with-start-intent', 'mcp-settings', 3,
    'desktop/src/react/__tests__/r08/r08-ui-settings-mcp.test.tsx',
    'R08 MCP 设置页：添加与批量导入（断言 4） 添加连接器提交真实动作与启动意图，成功后面板出现新连接器'),
  vitest('ui::mcp-settings.a3.bulk-import-per-row-partial', 'mcp-settings', 3,
    'desktop/src/react/__tests__/r08/r08-ui-settings-mcp.test.tsx',
    'R08 MCP 设置页：添加与批量导入（断言 4） 空表单不可提交；批量导入逐项反馈部分成功，不冒充全成功'),
  vitest('ui::mcp-settings.a4.remove-with-confirm', 'mcp-settings', 4,
    'desktop/src/react/__tests__/r08/r08-ui-settings-mcp.test.tsx',
    'R08 MCP 设置页：编辑/删除/启停/刷新（断言 5） 删除需确认，确认后提交真实移除动作；失败时保留连接器并显示错误'),
  vitest('ui::mcp-settings.a4.start-stop-refresh-actions-keep-credentials', 'mcp-settings', 4,
    'desktop/src/react/__tests__/r08/r08-ui-settings-mcp.test.tsx',
    'R08 MCP 设置页：编辑/删除/启停/刷新（断言 5） 启动、停止与刷新工具提交真实动作；停用不清凭据，成功后重读连接状态'),
  vitest('ui::mcp-settings.a5.agent-connector-tools-pinned', 'mcp-settings', 5,
    'desktop/src/react/__tests__/r08/r08-ui-settings-mcp.test.tsx',
    'R08 MCP 设置页：按助手权限/工具/置顶（断言 6） 按助手启停连接器并保存工具权限与置顶，动作携带当前助手身份'),
  vitest('ui::mcp-settings.a5.no-agent-refuses-detail', 'mcp-settings', 5,
    'desktop/src/react/__tests__/r08/r08-ui-settings-mcp.test.tsx',
    'R08 MCP 设置页：按助手权限/工具/置顶（断言 6） 无工具显示空目录；无助手时不伪造授权，打开详情被明确拒绝'),
  vitest('ui::mcp-settings.a6.oauth-login-open-external-poll-refresh', 'mcp-settings', 6,
    'desktop/src/react/__tests__/r08/r08-ui-settings-mcp.test.tsx',
    'R08 MCP 设置页：OAuth 登录/取消/退出（断言 7） 登录打开外链并轮询到 done 后刷新连接器并显示保存成功'),
  vitest('ui::mcp-settings.a6.oauth-cancel-post-only', 'mcp-settings', 6,
    'desktop/src/react/__tests__/r08/r08-ui-settings-mcp.test.tsx',
    'R08 MCP 设置页：OAuth 登录/取消/退出（断言 7） 取消只提交取消动作并关闭等待卡，不因取消本身重读状态'),
  vitest('ui::mcp-settings.a6.oauth-logout-reread', 'mcp-settings', 6,
    'desktop/src/react/__tests__/r08/r08-ui-settings-mcp.test.tsx',
    'R08 MCP 设置页：OAuth 登录/取消/退出（断言 7） 退出登录走统一动作并经真实状态刷新显示成功；启动失败不打开外链'),
  vitest('ui::mcp-settings.a6.oauth-start-failure-no-external', 'mcp-settings', 6,
    'desktop/src/react/__tests__/r08/r08-ui-settings-mcp.test.tsx',
    'R08 MCP 设置页：OAuth 登录/取消/退出（断言 7） 登录启动失败显示错误且不打开外链、不进入轮询'),

  // ── 安全设置页叶（4 条断言，0-based a0..a3）──
  vitest('ui::security.a0.container-cold-start-security-page', 'security', 0,
    'desktop/src/react/__tests__/r08/r08-ui-settings-container.test.tsx',
    'R08 设置容器初始化（MCP 设置页叶 & 安全设置页叶共用断言 1） 安全设置页冷启动：初始化完成后移除 mask 并渲染沙箱/备份/代理配置'),
  vitest('ui::security.a0.init-error-ready-with-log', 'security', 0,
    'desktop/src/react/__tests__/r08/r08-ui-settings-container.test.tsx',
    'R08 设置容器初始化（MCP 设置页叶 & 安全设置页叶共用断言 1） 初始化异常记录日志后仍 ready，并保留可操作的恢复入口（不装成成功空数据）'),
  vitest('ui::security.a0.fifteen-second-timeout-ready', 'security', 0,
    'desktop/src/react/__tests__/r08/r08-ui-settings-container.test.tsx',
    'R08 设置容器初始化（MCP 设置页叶 & 安全设置页叶共用断言 1） 15 秒无响应强制 ready 并移除 loading mask，杜绝无限白屏'),
  vitest('ui::security.a1.sandbox-switches-save-and-reread', 'security', 1,
    'desktop/src/react/__tests__/r08/r08-ui-settings-security.test.tsx',
    'R08 安全设置页：沙箱与沙箱网络（断言 1） 两个沙箱开关经每助手配置接口写入并重读当前助手合成配置视图'),
  vitest('ui::security.a1.uninitialized-loading-not-off', 'security', 1,
    'desktop/src/react/__tests__/r08/r08-ui-settings-security.test.tsx',
    'R08 安全设置页：沙箱与沙箱网络（断言 1） 状态未初始化时开关为 loading/禁用，不把未加载当关闭，也不发保存请求'),
  vitest('ui::security.a1.save-failure-restores-and-snapshot-failure', 'security', 1,
    'desktop/src/react/__tests__/r08/r08-ui-settings-security.test.tsx',
    'R08 安全设置页：沙箱与沙箱网络（断言 1） 保存失败恢复开关并提示错误；PUT 已成功但快照读取失败时如实报保存失败'),
  vitest('ui::security.a2.retention-and-backup-list', 'security', 2,
    'desktop/src/react/__tests__/r08/r08-ui-settings-security.test.tsx',
    'R08 安全设置页：检查点保留策略与备份列表（断言 2） 保留天数写入每助手配置并展开读取真实检查点列表'),
  vitest('ui::security.a2.empty-and-read-failure-explicit', 'security', 2,
    'desktop/src/react/__tests__/r08/r08-ui-settings-security.test.tsx',
    'R08 安全设置页：检查点保留策略与备份列表（断言 2） 无检查点显示空列表；读取失败显示显式错误并保留重试，不冒充空列表'),
  vitest('ui::security.a3.manual-proxy-save-and-notify', 'security', 3,
    'desktop/src/react/__tests__/r08/r08-ui-settings-security.test.tsx',
    'R08 安全设置页：网络代理模式与字段保存（断言 3） 手工代理保存 HTTP/HTTPS/WS/WSS/noProxy 并广播 network-proxy-changed、重读配置'),
  vitest('ui::security.a3.system-mode-and-save-failure-keeps-draft', 'security', 3,
    'desktop/src/react/__tests__/r08/r08-ui-settings-security.test.tsx',
    'R08 安全设置页：网络代理模式与字段保存（断言 3） 系统代理模式不渲染手工字段（保存仅模式）；保存失败提示并保留草稿，不广播'),

  // ── 快捷窗口叶（2 条断言，0-based a0..a1；真 Electron live 证据）──
  live('ui::quick-chat.a0.visible-compact-bounds', 'quick-chat', 0, 'quickchat', 'actual-production-show-visible-and-compact-bounds'),
  live('ui::quick-chat.a0.resize-increases-window', 'quick-chat', 0, 'quickchat', 'actual-production-resize-increases-window-and-resizable'),
  live('ui::quick-chat.a0.switch-acks-focus-main-then-hide', 'quick-chat', 0, 'quickchat', 'actual-owned-switch-ACK-focus-main-and-only-then-hide'),
  live('ui::quick-chat.a0.same-path-requires-real-service-focus', 'quick-chat', 0, 'quickchat', 'same-path-still-requires-real-service-focus'),
  live('ui::quick-chat.a1.missing-session-refuses-keeps-floating', 'quick-chat', 1, 'quickchat', 'actual-missing-session-refuses-and-keeps-floating-window'),
  live('ui::quick-chat.a1.destroyed-main-explicit-failure', 'quick-chat', 1, 'quickchat', 'actual-destroyed-main-window-explicit-failure-keeps-floating'),
  live('ui::quick-chat.a1.changed-owner-refuses-without-hide', 'quick-chat', 1, 'quickchat', 'changed-main-connection-owner-refuses-without-hide'),
  live('ui::quick-chat.a1.late-ack-cannot-hide-after-timeout', 'quick-chat', 1, 'quickchat', 'late-actual-renderer-ACK-cannot-hide-after-timeout'),

  // ── 独立复核补洞轮（未覆盖子句补齐；全部同一 producer/ui-cases.json）──
  // MCP a3：HTTP 200 混合回执逐行反馈，不弹全成功 toast。
  vitest('ui::mcp-settings.a3.bulk-partial-http200-mixed', 'mcp-settings', 3,
    'desktop/src/react/__tests__/r08/r08-ui-settings-mcp.test.tsx',
    'R08 MCP 设置页：批量成功/失败混合回执与权限保存失败（补充断言 4/6 未覆盖子句） 批量导入 HTTP 200 混合回执逐行呈现成功/runtimeApplied=false 事实，不弹全成功 toast'),
  // MCP a5：工具权限保存失败不得伪装授权已更新。
  vitest('ui::mcp-settings.a5.policy-save-failure-no-fake-grant', 'mcp-settings', 5,
    'desktop/src/react/__tests__/r08/r08-ui-settings-mcp.test.tsx',
    'R08 MCP 设置页：批量成功/失败混合回执与权限保存失败（补充断言 4/6 未覆盖子句） 工具权限与置顶保存失败显示错误且不伪装授权已更新'),
  // 安全 a2：无 agentId 时 loadSettingsConfig 零请求。
  vitest('ui::security.a2.no-agent-load-settings-zero-request', 'security', 2,
    'desktop/src/react/__tests__/r08/r08-ui-settings-guards.test.ts',
    'R08 安全设置页共用法守卫：loadSettingsConfig 请求边界 无 agentId 时 loadSettingsConfig 不发任何请求，并显式报 No settings agent selected'),
  // 安全 a3：经验未启用时不 GET experience。
  vitest('ui::security.a3.experience-disabled-zero-request', 'security', 3,
    'desktop/src/react/__tests__/r08/r08-ui-settings-guards.test.ts',
    'R08 安全设置页共用法守卫：loadSettingsConfig 请求边界 经验未启用时不 GET experience；显式启用后才发起 experience 读取'),
  // 安全 a4：代理通知抛错不得冒充完成。
  vitest('ui::security.a3.notify-failure-no-success', 'security', 3,
    'desktop/src/react/__tests__/r08/r08-ui-settings-security.test.tsx',
    'R08 安全设置页：代理通知失败不得冒充完成（补充断言 3 未覆盖子句） network-proxy-changed 通知抛错时仅记录未处理异常，不弹保存成功、不完成后续重读'),
  // 安全 a4：无可用连接（resourceKey 无法定位真实服务）不发网络请求。
  vitest('ui::security.a1.no-connection-zero-network-request', 'security', 1,
    'desktop/src/react/__tests__/r08/r08-ui-settings-guards.test.ts',
    'R08 安全设置页共用法守卫：loadSettingsConfig 请求边界 无可用连接（resourceKey 无法定位真实服务）时 loadSettingsConfig 不发出网络请求'),
  // chat a1：WS 拒绝/断线在页面内显式呈现。
  vitest('ui::chat.a1.ws-error-shown-in-page', 'chat', 1,
    'desktop/src/react/__tests__/r08/r08-ui-chat-page.test.tsx',
    'R08 聊天页：WS 拒绝/断线与恢复流状态（补充断言 2/3 未覆盖子句） WS 拒绝/断线事件在页面内显式呈现错误，不冒充消息成功'),
  // chat a2：恢复流改变运行状态且 UI 控件态一致。
  vitest('ui::chat.a2.resume-status-drives-control', 'chat', 2,
    'desktop/src/react/__tests__/r08/r08-ui-chat-page.test.tsx',
    'R08 聊天页：WS 拒绝/断线与恢复流状态（补充断言 2/3 未覆盖子句） 恢复流的运行状态经真实 ws 分发后与输入区控件态一致'),
];

function vitest(caseName, leaf, assertion, file, fullName) {
  return { case: caseName, leaf, assertion, evidence: { kind: 'vitest', file, fullName } };
}
function live(caseName, leaf, assertion, suite, check) {
  return { case: caseName, leaf, assertion, evidence: { kind: 'live', suite, check } };
}

const LEAF_NAMES = {
  chat: 'R00-T02-LA-81B217A7E581 F-D01-UI-UI-PAGE-CHAT',
  map: 'R00-T02-LA-8E2D44B48905 F-D02-UI-UI-PAGE-MAP',
  'mcp-settings': 'R00-T02-LA-B4BB2438E855 F-D03-UI-UI-SETTINGS-MCP',
  security: 'R00-T02-LA-DA5AD5C40098 F-D04-UI-UI-SETTINGS-SECURITY',
  'quick-chat': 'R00-T02-LA-A2C385A33493 F-D01-DESKTOP_BEHAVIOR-QUICK-CHAT-WINDOW',
};

const logLines = [];
function log(message) {
  logLines.push(message);
  process.stderr.write(`${message}\n`);
}

function runProcess(command, commandArgs, options = {}) {
  return new Promise(resolve => {
    const child = spawn(command, commandArgs, {
      cwd: ROOT,
      env: options.env || process.env,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', chunk => { stdout += String(chunk); });
    child.stderr.on('data', chunk => { stderr += String(chunk); });
    let timedOut = false;
    const timer = options.timeoutMs ? setTimeout(() => {
      timedOut = true;
      child.kill('SIGTERM');
      setTimeout(() => { try { child.kill('SIGKILL'); } catch { /* already gone */ } }, 10_000).unref();
    }, options.timeoutMs) : null;
    child.on('error', error => {
      if (timer) clearTimeout(timer);
      resolve({ code: -1, stdout, stderr: `${stderr}\n${String(error)}`, timedOut });
    });
    child.on('exit', code => {
      if (timer) clearTimeout(timer);
      resolve({ code: code ?? -1, stdout, stderr, timedOut });
    });
  });
}

async function runVitestGroup(group) {
  const reportPath = path.join(OUT, 'raw', `${group.name}-report.json`);
  const vitestArgs = ['vitest', 'run', ...group.files,
    '--reporter=verbose', '--reporter=json', `--outputFile.json=${reportPath}`];
  if (group.pattern) vitestArgs.push('-t', group.pattern);
  log(`\n$ ${NPX} ${vitestArgs.join(' ')}`);
  const result = await runProcess(NPX, vitestArgs, { timeoutMs: 600_000 });
  log(result.stdout.trimEnd());
  if (result.stderr.trim()) log(result.stderr.trimEnd());

  let report = null;
  try {
    report = JSON.parse(await readFile(reportPath, 'utf8'));
  } catch (error) {
    log(`[vitest:${group.name}] 无法读取 JSON 报告：${String(error)}`);
  }
  const assertionIndex = new Map();
  const filesSeen = new Set();
  if (report) {
    for (const testFile of report.testResults || []) {
      const relative = path.isAbsolute(testFile.name) ? path.relative(ROOT, testFile.name) : testFile.name;
      const normalized = relative.split(path.sep).join('/');
      filesSeen.add(normalized);
      for (const assertion of testFile.assertionResults || []) {
        assertionIndex.set(`${normalized}|${assertion.fullName}`, assertion.status);
      }
    }
  }
  const missingFiles = group.files.filter(file => !filesSeen.has(file));
  const failedUnmapped = report
    ? (report.testResults || []).flatMap(testFile => (testFile.assertionResults || [])
      .filter(assertion => assertion.status === 'failed')
      .map(assertion => `${path.basename(testFile.name)} :: ${assertion.fullName}`))
    : [];
  const groupOk = result.code === 0 && !!report && missingFiles.length === 0 && failedUnmapped.length === 0;
  log(`[vitest:${group.name}] exit=${result.code} report=${report ? 'yes' : 'no'} files=${filesSeen.size}/${group.files.length} failed=${failedUnmapped.length}`);
  return {
    name: group.name,
    exitCode: result.code,
    timedOut: result.timedOut,
    reportFound: !!report,
    missingFiles,
    failedUnmapped,
    ok: groupOk,
    statusFor(caseEntry) {
      return assertionIndex.get(`${caseEntry.evidence.file}|${caseEntry.evidence.fullName}`) || null;
    },
  };
}

function liveEnv(extra) {
  const env = { ...process.env, ...extra };
  delete env.ELECTRON_RUN_AS_NODE;
  delete env.ELECTRON_NO_ATTACH_CONSOLE;
  delete env.ELECTRON_ENABLE_LOGGING;
  delete env.VITEST;
  delete env.LINGXI_HOME;
  return env;
}

async function runLiveSuite(suite, cases) {
  if (NO_LIVE) {
    log(`[live:${suite}] --no-live：不运行，按失败记录`);
    return { name: suite, ok: false, reason: 'skipped by --no-live', checks: new Set(), exitCode: null };
  }
  try {
    await access(SERVICE_BIN);
  } catch {
    log(`[live:${suite}] 缺少 Rust 服务二进制：${SERVICE_BIN}`);
    return { name: suite, ok: false, reason: `missing service binary ${SERVICE_BIN}`, checks: new Set(), exitCode: null };
  }
  const evidenceDir = path.join(OUT, 'live', suite);
  // live 运行器要求证据目录不存在（mkdir 非 exist_ok）；每次运行前清掉上一轮
  // 的同名目录，避免陈旧 checks 回执被当成本轮结果。
  await rm(evidenceDir, { recursive: true, force: true });
  let script;
  let env;
  let checksFile;
  if (suite === 'quickchat') {
    script = path.join(ROOT, 'tests', 'rust-native', 'quickchat-live.py');
    env = liveEnv({
      R06_QUICKCHAT_EVIDENCE: evidenceDir,
      R06_QUICKCHAT_SERVICE_BIN: SERVICE_BIN,
    });
    checksFile = path.join(evidenceDir, 'result.json');
  } else {
    script = path.join(ROOT, 'tests', 'rust-native', 'session-map-live.py');
    env = liveEnv({
      R06_SESSION_EVIDENCE: evidenceDir,
      R06_SESSION_SERVICE_BIN: SERVICE_BIN,
      R06_SESSION_UI: '1',
      R06_NODE_BIN: process.execPath,
    });
    checksFile = path.join(evidenceDir, 'map-ui-result.json');
  }
  log(`\n$ ${PYTHON} ${path.relative(ROOT, script)}  # evidence=${evidenceDir}`);
  const result = await runProcess(PYTHON, [script], { env, timeoutMs: suite === 'quickchat' ? 180_000 : 360_000 });
  const tail = (result.stdout + result.stderr).trim().split('\n').slice(-40).join('\n');
  log(tail);
  let checks = new Set();
  let status = null;
  try {
    const raw = JSON.parse(await readFile(checksFile, 'utf8'));
    status = raw.status;
    if (Array.isArray(raw.checks)) {
      checks = new Set(raw.checks.map(check => (typeof check === 'string' ? check : check?.name)).filter(Boolean));
    }
  } catch (error) {
    log(`[live:${suite}] 无法读取 checks 回执 ${checksFile}：${String(error)}`);
  }
  const ok = result.code === 0 && status === 'PASS';
  log(`[live:${suite}] exit=${result.code} status=${status} checks=${checks.size} ok=${ok}`);
  return { name: suite, ok, exitCode: result.code, status, checks, timedOut: result.timedOut, evidenceDir };
}

async function main() {
  await mkdir(path.join(OUT, 'raw'), { recursive: true });
  log(`R08 UI 逐断言客户端证据生产 — out=${OUT}`);
  log(`node=${process.version} service=${SERVICE_BIN}`);

  const groups = [
    { name: 'r08-new-ui', files: NEW_UI },
    { name: 'r08-existing-ui', files: EXISTING_UI },
    { name: 'r08-composer-send', files: COMPOSER, pattern: COMPOSER_PATTERN },
  ];
  const groupResults = [];
  for (const group of groups) {
    // eslint-disable-next-line no-await-in-loop -- 串行跑，避免 vitest worker 互抢资源
    groupResults.push(await runVitestGroup(group));
  }

  const liveSuites = new Map();
  if (!NO_LIVE) {
    for (const suite of ['quickchat', 'session-map']) {
      // eslint-disable-next-line no-await-in-loop -- 两个 Electron live 串行，避免窗口/焦点互抢
      liveSuites.set(suite, await runLiveSuite(suite, CASES.filter(entry => entry.evidence.suite === suite)));
    }
  } else {
    for (const suite of ['quickchat', 'session-map']) {
      liveSuites.set(suite, { name: suite, ok: false, checks: new Set(), exitCode: null, reason: '--no-live' });
    }
  }

  const fileToGroup = new Map();
  for (const result of groupResults) {
    for (const file of [].concat(result.name === 'r08-composer-send' ? COMPOSER : result.name === 'r08-new-ui' ? NEW_UI : EXISTING_UI)) {
      fileToGroup.set(file, result);
    }
  }

  const cases = CASES.map(entry => {
    let actual = 0;
    let reason = '';
    if (entry.evidence.kind === 'vitest') {
      const group = fileToGroup.get(entry.evidence.file);
      const status = group ? group.statusFor(entry) : null;
      actual = status === 'passed' ? 1 : 0;
      if (status === null) reason = 'test missing from vitest report';
      else if (status !== 'passed') reason = `test status ${status}`;
      if (group && !group.ok && status === 'passed') reason = 'suite run failed (unmapped failure or metadata problem)';
    } else {
      const suite = liveSuites.get(entry.evidence.suite);
      actual = suite && suite.ok && suite.checks.has(entry.evidence.check) ? 1 : 0;
      if (!suite) reason = 'live suite not run';
      else if (!suite.ok) reason = `live suite failed: ${suite.reason || `exit ${suite.exitCode}`}`;
      else if (!suite.checks.has(entry.evidence.check)) reason = 'live check missing';
    }
    return { case: entry.case, leaf: entry.leaf, assertion: entry.assertion, expect: 1, actual, ok: actual === 1, reason };
  });

  const leafStats = {};
  for (const item of cases) {
    leafStats[item.leaf] = leafStats[item.leaf] || { leaf: item.leaf, leafName: LEAF_NAMES[item.leaf], assertions: new Set(), cases: 0, passed: 0, failed: 0 };
    leafStats[item.leaf].assertions.add(item.assertion);
    leafStats[item.leaf].cases += 1;
    if (item.ok) leafStats[item.leaf].passed += 1;
    else leafStats[item.leaf].failed += 1;
  }

  const casesOk = cases.every(item => item.ok);
  const groupsOk = groupResults.every(group => group.ok);
  const liveOk = [...liveSuites.values()].every(suite => suite.ok);
  const exitCode = casesOk && groupsOk && liveOk ? 0 : 1;

  const uiCases = {
    schema: 'lingxi.leaf-case-results.v1',
    cases: cases.map(item => ({ case: item.case, expect: item.expect, actual: item.actual, ok: item.ok })),
  };
  await writeFile(path.join(OUT, 'ui-cases.json'), `${JSON.stringify(uiCases, null, 2)}\n`);

  const receipt = {
    schema: 'lingxi.r08.ui-pages-receipt.v1',
    stage: 'R08',
    producer: 'scripts/rust-tauri/r08_ui_pages.mjs',
    command: `node scripts/rust-tauri/r08_ui_pages.mjs --out ${path.relative(ROOT, OUT) || OUT}`,
    node: process.version,
    generatedAt: new Date().toISOString(),
    exitCode,
    cases: { total: cases.length, passed: cases.filter(item => item.ok).length, failed: cases.filter(item => !item.ok).length },
    leaves: Object.values(leafStats).map(item => ({
      leaf: item.leaf,
      leafName: item.leafName,
      assertions: item.assertions.size,
      cases: item.cases,
      passed: item.passed,
      failed: item.failed,
    })),
    vitestGroups: groupResults.map(group => ({
      name: group.name,
      exitCode: group.exitCode,
      ok: group.ok,
      reportFound: group.reportFound,
      missingFiles: group.missingFiles,
      failedUnmapped: group.failedUnmapped,
    })),
    liveSuites: [...liveSuites.values()].map(suite => ({
      name: suite.name,
      exitCode: suite.exitCode,
      status: suite.status,
      ok: suite.ok,
      checks: suite.checks.size,
      reason: suite.reason,
      evidenceDir: suite.evidenceDir ? path.relative(ROOT, suite.evidenceDir) : undefined,
    })),
    caseDetails: cases.map(item => ({
      case: item.case,
      leaf: item.leaf,
      assertion: item.assertion,
      evidence: CASES.find(entry => entry.case === item.case).evidence,
      actual: item.actual,
      ok: item.ok,
      reason: item.reason,
    })),
  };
  await writeFile(path.join(OUT, 'result.json'), `${JSON.stringify(receipt, null, 2)}\n`);
  log(`\n写入 ${path.join(OUT, 'ui-cases.json')}（${cases.length} cases, ${cases.filter(item => item.ok).length} ok）`);
  log(`写入 ${path.join(OUT, 'result.json')}`);
  if (exitCode !== 0) {
    for (const item of cases.filter(entry => !entry.ok)) log(`FAIL ${item.case} :: ${item.reason}`);
    log(`EXIT 1（casesOk=${casesOk} groupsOk=${groupsOk} liveOk=${liveOk}）`);
  } else {
    log('EXIT 0 — 全部 case ok（vitest 与 live 证据均为真实通过）');
  }
  await writeFile(path.join(OUT, 'output.log'), `${logLines.join('\n')}\n`);
  process.exit(exitCode);
}

main().catch(async error => {
  log(`FATAL ${String(error?.stack || error)}`);
  try { await mkdir(OUT, { recursive: true }); } catch { /* best effort */ }
  await writeFile(path.join(OUT, 'output.log'), `${logLines.join('\n')}\n`);
  await writeFile(path.join(OUT, 'ui-cases.json'), `${JSON.stringify({
    schema: 'lingxi.leaf-case-results.v1',
    cases: CASES.map(entry => ({ case: entry.case, expect: 1, actual: 0, ok: false })),
  }, null, 2)}\n`);
  process.exit(1);
});
