/**
 * 工具行文案解析。
 *
 * 进程区每条工具调用只有这一个取值点，键形如 `tool.<工具名>.<相位>`。
 * 工具名对不上就静默落到兜底文案，运行时不报错，所以内置名单和语言包的一致性
 * 由 tests/tool-label-coverage.test.ts 对账守住。
 */

import type { ToolCall } from '../stores/chat-types';

export type ToolPhase = 'running' | 'done' | 'failed';
export type ToolStatus = NonNullable<ToolCall['status']>;

/** 这两个工具复用别的工具的文案。 */
export const TOOL_LABEL_ALIASES: Record<string, string> = {
  exec_command: 'bash',
  write_stdin: 'terminal',
};

/**
 * session 一个工具管三件不同的事，一套文案盖不住：read/list 是查看别的会话，
 * send/create 只是拟了一张待确认的草稿卡，那一刻消息还没发出去。用一套
 * "联系上了"的说法会在卡片还等着确认的时候就宣布结果。
 */
export const SESSION_ACTION_LABEL_KEYS: Record<string, string> = {
  send: 'session_send',
  create: 'session_create',
};

export const KNOWLEDGE_RESEARCH_TOOL_NAMES: ReadonlySet<string> = new Set([
  'knowledge_research_plan', 'knowledge_research_round', 'knowledge_research_worker',
  'knowledge_research_progress', 'knowledge_research_review', 'knowledge_research_synthesis',
]);

function labelKeyFor(name: string, args?: Record<string, unknown>): string {
  const aliased = TOOL_LABEL_ALIASES[name];
  if (aliased) return aliased;
  if (name === 'session') {
    const action = typeof args?.action === 'string' ? args.action : '';
    return SESSION_ACTION_LABEL_KEYS[action] ?? 'session';
  }
  return name;
}

/**
 * 随 Lingxi 一起分发的内置插件工具。
 *
 * 运行时工具名统一是 `<pluginId>_<tool>`，这里的名字带全前缀；同前缀名在
 * BUILTIN_TOOL_NAMES 里，所以内置插件不会被 isExternalTool 误判成第三方工具。
 *
 * 这些工具当前由卡片承载、不进进程区（见 utils/tool-call-visibility.ts）。仍登记
 * 文案是对账口径的一部分：工具可见性由 args 决定，卡片形态一变，没有文案的行就会
 * 直接露出裸英文工具名。
 */
export const BUNDLED_PLUGIN_TOOL_NAMES: ReadonlySet<string> = new Set([
  'media_generate-image', 'media_generate-video', 'media_describe-options', 'media_get-guide',
  'beautify_create-cover', 'beautify_apply-cover-candidate', 'beautify_get-cover-style-guide',
  'beautify_get-html-style-guide', 'beautify_list-capabilities',
  'office_read-document', 'office_html-to-pdf', 'office_list-capabilities',
]);

/**
 * 一方工具名。插件工具由 PluginManager 注册成 `<pluginId>_<tool>`，MCP 工具是
 * `mcp_<tool>`，都不在这张表里，因此查不到专属文案时能落到插件兜底而不是通用兜底。
 *
 * 不走"剥掉前缀再查一次"那条捷径：第三方插件里叫 read / write 的工具会直接撞上
 * 内置工具的文案，把别人干的事说成是读写本地文件。
 *
 * 后端哪天在工具调用事件里带上 pluginId，这张表连同 isExternalTool 就能整块删掉。
 */
export const BUILTIN_TOOL_NAMES: ReadonlySet<string> = new Set([
  'read', 'write', 'edit', 'grep', 'find', 'ls', 'bash', 'terminal', 'materialize',
  'exec_command', 'write_stdin',
  'search_memory', 'pin_memory', 'unpin_memory', 'recall_experience', 'record_experience', 'tenet_propose',
  'web_search', 'web_fetch', 'todo_write', 'automation', 'stage_files', 'file', 'channel',
  'browser', 'computer', 'install_skill', 'notify', 'stop_task', 'update_settings',
  'session_folders', 'subagent', 'subagent_reply', 'subagent_close', 'workflow',
  'check_pending_tasks', 'loop_control', 'current_status', 'session', 'knowledge_search', 'knowledge_read',
  'knowledge_outline', 'knowledge_grep', 'knowledge_manage',
  'knowledge_think', 'knowledge_read_part', 'knowledge_supplement',
  'knowledge_answer', 'knowledge_local_search',
  ...KNOWLEDGE_RESEARCH_TOOL_NAMES,
  ...BUNDLED_PLUGIN_TOOL_NAMES,
  'hana_card_guide', 'show_card',
  'channel_read_context', 'channel_reply', 'channel_pass',
  // 已下线但有历史调用记录，回看旧会话时仍要能落到一方文案而不是插件兜底
  'create_artifact', 'dm', 'present_files',
]);

/**
 * 工具行主标签：`messageActivity.labels.<键>` 短标签。
 *
 * 与 `tool.<工具名>.<相位>` 分开：那一套是带 emoji、agent 名和结果口吻的整句，
 * 留给知识研究回放等旧分支；工具行只要一个短词（「回想」「查资料」）。
 *
 * 这张表是"每个内置工具都有中文短标签"的唯一登记点，五语言齐不齐由
 * tests/tool-label-coverage.test.ts 对账守住。
 */
export const ACTIVITY_LABEL_KEYS: Readonly<Record<string, string>> = {
  // Pi SDK 沙盒工具
  read: 'read', write: 'write', edit: 'edit', grep: 'grep', find: 'find', ls: 'ls',
  bash: 'terminal', terminal: 'terminal', exec_command: 'terminal', write_stdin: 'terminal',
  materialize: 'materialize',
  // 记忆
  search_memory: 'search_memory', pin_memory: 'pin_memory', unpin_memory: 'unpin_memory',
  recall_experience: 'recall_experience', record_experience: 'record_experience', tenet_propose: 'tenet_propose',
  // 知识
  knowledge_search: 'knowledge_search', knowledge_read: 'knowledge_read', knowledge_outline: 'knowledge_outline',
  knowledge_grep: 'knowledge_grep', knowledge_manage: 'knowledge_manage', knowledge_think: 'knowledge_think',
  knowledge_read_part: 'knowledge_read_part', knowledge_supplement: 'knowledge_supplement',
  knowledge_answer: 'knowledge_answer', knowledge_local_search: 'knowledge_local_search',
  knowledge_research_plan: 'knowledge_research', knowledge_research_round: 'knowledge_research',
  knowledge_research_worker: 'knowledge_research', knowledge_research_progress: 'knowledge_research',
  knowledge_research_review: 'knowledge_research', knowledge_research_synthesis: 'knowledge_research',
  // 会话
  session: 'session',
  // 网页
  web_search: 'web_search', web_fetch: 'web_fetch',
  // Hub 频道
  channel: 'channel', channel_read_context: 'channel_read_context', channel_reply: 'channel_reply',
  channel_pass: 'channel_pass',
  // 通知 / 浏览器 / 电脑
  notify: 'notify', browser: 'browser', computer: 'computer',
  // 文件
  file: 'file',
  // Agent 自带
  automation: 'automation', stop_task: 'stop_task', session_folders: 'session_folders',
  check_pending_tasks: 'check_pending_tasks', loop_control: 'loop_control', current_status: 'current_status',
  subagent_reply: 'subagent_reply', subagent_close: 'subagent_close',
  // 卡片承载（不进进程区，仍要登记文案；键形如 <pluginId>_<tool>）
  'create_artifact': 'create_artifact', 'dm': 'dm',
  'media_generate-image': 'media_generate-image', 'media_generate-video': 'media_generate-video',
  'media_describe-options': 'media_describe-options', 'media_get-guide': 'media_get-guide',
  'beautify_create-cover': 'beautify_create-cover',
  'beautify_apply-cover-candidate': 'beautify_apply-cover-candidate',
  'beautify_get-cover-style-guide': 'beautify_get-cover-style-guide',
  'beautify_get-html-style-guide': 'beautify_get-html-style-guide',
  'beautify_list-capabilities': 'beautify_list-capabilities',
  'office_read-document': 'office_read-document', 'office_html_to-pdf': 'office_html-to-pdf',
  'office_list-capabilities': 'office_list-capabilities',
  // 家族词：查不到短标签时按工具是不是一方工具分档
  _tool: 'tool',
  _plugin: '_plugin',
};

/**
 * 工具行主标签的解析顺序，全局只有这一处：
 *   形态名（技能）→ 一方短标签 → 插件/MCP 家族词（「扩展」）→ 通用词（「工具」）。
 *
 * 任何分支都不返回工具本名：裸英文工具名（search_memory、mcp_deep-search）不许当
 * 行主标签，原工具名只保留在行上的 data-tool、悬停提示与完整调用弹窗里。
 */
export function activityLabel(
  name: string,
  options?: { skill?: boolean; todoTitle?: string },
): string {
  const translate = window.t ?? ((key: string) => key);
  if (options?.skill) return translate('messageActivity.labels.skill');
  if (options?.todoTitle) return options.todoTitle;
  const aliased = TOOL_LABEL_ALIASES[name] ?? name;
  const key = ACTIVITY_LABEL_KEYS[aliased];
  if (key) return translate(`messageActivity.labels.${key}`);
  // 插件/MCP 工具名（`mcp_<tool>`、`<pluginId>_<tool>`）不可预知，逐工具配不出短标签，
  // 所以整个家族统一用一个词（labels._plugin「扩展」）；内置工具没短标签时落 labels.tool。
  return translate(`messageActivity.labels.${isExternalTool(name) ? '_plugin' : 'tool'}`);
}

export function isExternalTool(name: string): boolean {
  return !BUILTIN_TOOL_NAMES.has(name) && name.includes('_');
}

function resolveToolCopy(key: string, phase: ToolPhase, vars: Record<string, string>): string | null {
  const path = `tool.${key}.${phase}`;
  const value = window.t?.(path, vars);
  return value && value !== path ? value : null;
}

export function getToolLabel(
  name: string,
  phase: ToolPhase,
  agentName: string,
  args?: Record<string, unknown>,
): string {
  const vars: Record<string, string> = { name: agentName };
  if (KNOWLEDGE_RESEARCH_TOOL_NAMES.has(name)) {
    for (const field of ['round', 'maxRounds', 'count', 'completed', 'total']) {
      const value = args?.[field];
      vars[field] = typeof value === 'number' && Number.isSafeInteger(value) && value >= 0 ? String(value) : '?';
    }
  }
  const key = labelKeyFor(name, args);
  return resolveToolCopy(key, phase, vars)
    ?? (isExternalTool(name) ? resolveToolCopy('_plugin', phase, vars) : null)
    ?? resolveToolCopy('_fallback', phase, vars)
    ?? name;
}

/**
 * session 工具的目标会话，用来填工具行右侧那格。
 *
 * 只认 args 里的 sessionId，再从按 sessionId 索引的容器里查，不从当前焦点推导归属。
 * 查不到（会话已归档或不在列表里）返回 null，由调用方退回 id 短尾，不猜。
 */
export interface SessionTargetState {
  sessions?: Array<{ sessionId?: string | null; title?: string | null; agentName?: string | null }>;
  sessionLocatorsById?: Record<string, { path: string | null }>;
}

function targetSessionId(args?: Record<string, unknown>): string | null {
  const raw = args?.sessionId;
  return typeof raw === 'string' && raw.trim() ? raw.trim() : null;
}

export function sessionToolTargetName(
  state: SessionTargetState,
  args?: Record<string, unknown>,
): string | null {
  // create 还没有目标会话，args 里给的是要派给谁
  if (args?.action === 'create') {
    const agent = args?.agent;
    return typeof agent === 'string' && agent.trim() ? agent.trim() : null;
  }
  const sessionId = targetSessionId(args);
  if (!sessionId) return null;
  const found = (state.sessions || []).find((item) => item?.sessionId === sessionId);
  if (!found) return null;
  const parts = [found.agentName, found.title].filter((v): v is string => Boolean(v && v.trim()));
  return parts.length ? parts.join(' · ') : null;
}

export function sessionToolTargetPath(
  state: SessionTargetState,
  args?: Record<string, unknown>,
): string | null {
  if (args?.action === 'create') return null;
  const sessionId = targetSessionId(args);
  if (!sessionId) return null;
  return state.sessionLocatorsById?.[sessionId]?.path || null;
}

/** unknown 归到 done：工具已经不转了，说"正在忙碌"会一直挂着。 */
export function phaseForStatus(status: ToolStatus): ToolPhase {
  if (status === 'running') return 'running';
  if (status === 'failed') return 'failed';
  return 'done';
}

/**
 * 消息流底部「正在做什么」状态行的文案键（`chat.running.*`）。
 *
 * 与 `tool.<name>.<phase>` 分开：工具行文案自带 emoji、agent 名与结果口吻，
 * 适合卡片行；状态行只要一段干净的动名词短句（「正在读取文件」），后面紧跟
 * 秒表，所以按操作类别归一，而不是一工具一句。
 *
 * `thinking`（未封口思考 / 首个事件还没到）与 `writing`（在流正文、没有思考也
 * 没有工具）不是工具名派生的，故不在 RUNNING_STATUS_BY_TOOL 表里。
 *
 * 归类只用精确工具名，不做前缀剥离：第三方插件里叫 read 的工具不能被说成
 * 在读写本地文件（与 getToolLabel 同一立场），一律落到 `tool`。
 */
export type RunningStatusKey =
  | 'working' | 'thinking' | 'writing' | 'reading' | 'searching' | 'editing' | 'command'
  | 'web' | 'memory' | 'knowledge' | 'tool';

/** 只有状态行动态聚合时才用到的键：没有单个工具名对应它们。 */
export type RunningStatusToolKey = Exclude<RunningStatusKey, 'thinking' | 'writing'>;

/**
 * 工具名 → 状态行类别的唯一映射表。
 * 导出供对账测试使用：语言包里的每个 `chat.running.*` 叶子都必须被某条映射
 * （或 thinking / writing 两个非工具态）覆盖，不能出现查不到的孤儿文案。
 */
export const RUNNING_STATUS_BY_TOOL: Readonly<Record<string, RunningStatusToolKey>> = {
  read: 'reading', materialize: 'reading',
  grep: 'searching', find: 'searching', ls: 'searching',
  edit: 'editing', write: 'editing',
  bash: 'command', terminal: 'command', exec_command: 'command', write_stdin: 'command',
  web_search: 'web', web_fetch: 'web',
  search_memory: 'memory', pin_memory: 'memory', unpin_memory: 'memory',
  recall_experience: 'memory', record_experience: 'memory', tenet_propose: 'memory',
  knowledge_search: 'knowledge', knowledge_read: 'knowledge', knowledge_outline: 'knowledge',
  knowledge_grep: 'knowledge', knowledge_manage: 'knowledge', knowledge_think: 'knowledge',
  knowledge_read_part: 'knowledge', knowledge_supplement: 'knowledge',
  knowledge_answer: 'knowledge', knowledge_local_search: 'knowledge',
  ...Object.fromEntries([...KNOWLEDGE_RESEARCH_TOOL_NAMES].map(name => [name, 'knowledge' as const])),
};

export function runningStatusKey(name: string): RunningStatusToolKey {
  return RUNNING_STATUS_BY_TOOL[name] ?? 'tool';
}
