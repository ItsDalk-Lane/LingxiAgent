import { describe, expect, it } from 'vitest';
import fs from 'node:fs';
import path from 'node:path';
import {
  ACTIVITY_LABEL_KEYS, BUILTIN_TOOL_NAMES, BUNDLED_PLUGIN_TOOL_NAMES, TOOL_LABEL_ALIASES as RUNTIME_ALIASES,
  SESSION_ACTION_LABEL_KEYS, activityLabel,
  isExternalTool, phaseForStatus, sessionToolTargetName, sessionToolTargetPath,
} from '../desktop/src/react/utils/tool-label';

/**
 * 工具行文案对账。
 *
 * 进程区每条工具调用的文案只有一个取值点（ToolGroupBlock 的 getToolLabel），
 * 键形如 `tool.<工具名>.<相位>`。工具名对不上就静默掉进 `tool._fallback`，
 * 用户看到的是"正在忙碌中…"。这类漏配在运行时不报错，只能靠对账守住。
 *
 * 两个方向都要查：
 *   1. 已登记的工具在五个语言包里三相位齐全；
 *   2. 源码里新冒出来的工具必须显式登记或显式豁免，防止清单本身过期。
 */

const localesDir = path.join(process.cwd(), 'desktop/src/locales');
const locales = ['en', 'zh', 'zh-TW', 'ja', 'ko'];
const phases = ['running', 'done', 'failed'];

/** UI 侧把这两个工具名映射到别的文案键上，对账要跟着映射走。 */
const TOOL_LABEL_ALIASES: Record<string, string> = {
  exec_command: 'bash',
  write_stdin: 'terminal',
};

/** 需要文案的工具名（含插件工具的 `<pluginId>_<tool>` 全名）。 */
const LABELED_TOOL_NAMES = [
  // Pi SDK 沙盒工具
  'read', 'write', 'edit', 'grep', 'find', 'ls', 'bash', 'terminal', 'materialize',
  // Agent 自带
  'search_memory', 'pin_memory', 'unpin_memory', 'recall_experience', 'record_experience', 'tenet_propose',
  'web_search', 'web_fetch', 'todo_write', 'automation', 'stage_files', 'file', 'channel',
  'browser', 'computer', 'install_skill', 'notify', 'stop_task', 'update_settings',
  'session_folders', 'subagent', 'subagent_reply', 'subagent_close', 'workflow',
  'check_pending_tasks', 'loop_control', 'current_status', 'session', 'knowledge_search', 'knowledge_read',
  'knowledge_think', 'knowledge_read_part', 'knowledge_supplement',
  'knowledge_answer', 'knowledge_local_search',
  'knowledge_research_plan', 'knowledge_research_round', 'knowledge_research_worker',
  'knowledge_research_progress', 'knowledge_research_review', 'knowledge_research_synthesis',
  'knowledge_outline', 'knowledge_grep', 'knowledge_manage',
  'hana_card_guide', 'show_card',
  // Hub 频道
  'channel_read_context', 'channel_reply', 'channel_pass',
  // 内置插件（PluginManager 注册时统一加 `<pluginId>_` 前缀）
  'media_generate-image', 'media_generate-video', 'media_describe-options', 'media_get-guide',
  'beautify_create-cover', 'beautify_apply-cover-candidate', 'beautify_get-cover-style-guide',
  'beautify_get-html-style-guide', 'beautify_list-capabilities',
  'office_read-document', 'office_html-to-pdf', 'office_list-capabilities',
  // 已下线但历史 JSONL 里仍有调用记录，回看旧会话时要能正常渲染
  'create_artifact', 'dm', 'present_files',
];

/**
 * 工具行主标签短文案覆盖的工具名。
 *
 * 取运行时的 ACTIVITY_LABEL_KEYS（登记表）加别名键：别名工具（exec_command /
 * write_stdin）在表里指向 terminal，本身不需要独立文案键，但必须有短标签可渲染。
 * 由登记表派生而不是再抄一份清单——抄一份就会出现"新工具只补了渲染侧、忘了对账"
 * 的缝隙。
 */
const ACTIVITY_LABEL_TOOL_NAMES = [
  ...new Set([
    ...Object.keys(ACTIVITY_LABEL_KEYS).filter((name) => !name.startsWith('_')),
    ...Object.keys(RUNTIME_ALIASES),
  ]),
];

/** 家族词 / 通用兜底的键：没有对应工具名，但五语言必须都有。 */
const ACTIVITY_FAMILY_LABEL_KEYS = Object.keys(ACTIVITY_LABEL_KEYS).filter((name) => name.startsWith('_'));

/**
 * 短标签对账豁免：这些工具有独立的面板标题，不走 messageActivity.labels。
 * todo_write 的行标签固定取 `todoPanel.title`（「任务」），由 ToolGroupBlock 的
 * 渲染用例守住，不在这里重复要求一个永远用不上的键。
 */
const ACTIVITY_LABEL_EXEMPT_TOOL_NAMES = new Set(['todo_write']);

/**
 * 已下线、只可能出现在历史 JSONL 里的工具：要有行短标签（旧会话回看时仍会渲染成
 * 工具行），但不要求 `tool.*` 那套整句文案。
 *
 * `present_files` 不在 lib/tools 注册表里，历史上也没有配过三相位文案；按拍板
 * 旧 `tool.*` 文案原样保留、不新增、不接回，所以这里显式豁免而不是补文案。
 */
const LEGACY_TOOL_NAMES = new Set(['present_files']);

/**
 * 文案键但不是工具名：同一个工具按 action 分出来的档位。
 * 它们要有完整三相位，但不进内置工具名单（那张表是拿工具名判断内外部用的）。
 */
const ACTION_LABEL_KEYS = ['session_send', 'session_create'];

/**
 * 不进进程区、因而不需要文案的工具。
 * 加新条目前先确认它真的不会出现在会话时间线里。
 */
const UNLABELED_TOOL_NAMES = new Set([
  'structured_output',              // workflow 内部结构化输出
  'jian_update_status',             // desk 心跳
  'knowledge_research_update',     // 隔离调查工具由聚合进度卡承接
  'knowledge_research_finish',
  'knowledge_delegate',
  'knowledge_coverage_read',       // 仅完整性隔离工作会话内使用，主会话显示聚合研究进度
  'knowledge_completeness_mark',
  'patrol_update_log',              // desk 巡检
  'hana',                           // MCP client 自我标识，非 agent 工具
  'stop', 'new', 'reset', 'rc', 'exitrc', 'apply', 'confirm', 'reject', 'compact', 'loop',
  'fd', 'ripgrep',                  // 搜索二进制下载配置，非 agent 工具
  'todo',                           // todo_write 的历史别名
]);

function loadLocale(name: string): Record<string, any> {
  return JSON.parse(fs.readFileSync(path.join(localesDir, `${name}.json`), 'utf8'));
}

/** 按 `a.b.c` 取值；缺键返回 undefined，好让调用方区分"没翻译"和"翻译成了空串"。 */
function loadKey(data: Record<string, any>, key: string): unknown {
  return key.split('.').reduce<any>((node, part) => (node == null ? undefined : node[part]), data);
}

/** 从源码里抓工具注册名，粗粒度但足以发现"新工具没登记"。 */
function scanRegisteredToolNames(): Set<string> {
  const sources = [
    ...fs.readdirSync(path.join(process.cwd(), 'lib/tools'))
      .filter((f) => f.endsWith('.ts'))
      .map((f) => path.join('lib/tools', f)),
    'lib/exec-command/tool.ts',
    'lib/memory/memory-search.ts',
    'lib/resource-io/materialize-tool.ts',
    'hub/channel-router.ts',
  ];
  const found = new Set<string>();
  for (const rel of sources) {
    const abs = path.join(process.cwd(), rel);
    if (!fs.existsSync(abs)) continue;
    const text = fs.readFileSync(abs, 'utf8');
    for (const m of text.matchAll(/^\s{2,6}name: "([a-z][a-z0-9_]*)",$/gm)) {
      found.add(m[1]);
    }
  }
  return found;
}

describe('工具行文案对账', () => {
  for (const locale of locales) {
    it(`${locale}.json 本地快速检索提示、结果与时限文案完整`, () => {
      const data = loadLocale(locale);
      for (const key of ['knowledgeModeFastHint', 'knowledgeModeDetailedHint']) {
        expect(data.input[key]).toBeTypeOf('string');
        expect(data.input[key].trim()).not.toBe('');
      }
      for (const key of ['knowledgeLocalEvidenceFound', 'knowledgeFastSummary']) {
        expect(data.chat[key]).toContain('{n}');
        expect(data.chat[key]).toContain('{ms}');
      }
      expect(data.chat.knowledgeFastDeadlineExceeded.trim()).not.toBe('');
    });
    it(`${locale}.json 为每个已登记工具提供三相位文案`, () => {
      const tool = loadLocale(locale).tool ?? {};
      const missing: string[] = [];
      for (const name of [...LABELED_TOOL_NAMES, ...ACTION_LABEL_KEYS]) {
        if (LEGACY_TOOL_NAMES.has(name)) continue;
        const key = TOOL_LABEL_ALIASES[name] ?? name;
        for (const phase of phases) {
          const value = tool[key]?.[phase];
          if (typeof value !== 'string' || !value.trim()) missing.push(`tool.${key}.${phase}`);
        }
      }
      expect(missing, `${locale}.json 缺工具文案`).toEqual([]);
    });
    it(`${locale}.json 为每个内置工具提供行短标签`, () => {
      // 硬指标：工具行主标签必须是文案词。缺键时 activityLabel 会落到通用兜底
      // （或裸键名），界面就会出现"每个冷门工具都叫工具"或裸英文工具名。
      const labels = loadLocale(locale).messageActivity?.labels ?? {};
      const missing: string[] = [];
      const rawNames: string[] = [];
      for (const key of ACTIVITY_FAMILY_LABEL_KEYS) {
        const value = labels[ACTIVITY_LABEL_KEYS[key]];
        if (typeof value !== 'string' || !value.trim()) missing.push(`messageActivity.labels.${ACTIVITY_LABEL_KEYS[key]}`);
      }
      for (const name of ACTIVITY_LABEL_TOOL_NAMES) {
        if (ACTIVITY_LABEL_EXEMPT_TOOL_NAMES.has(name)) continue;
        const key = ACTIVITY_LABEL_KEYS[name] ?? name;
        const value = labels[key];
        if (typeof value !== 'string' || !value.trim()) missing.push(`messageActivity.labels.${key}`);
        // 英文界面也不许把工具本名当标签：en 里 `read` 叫 Read、`ls` 叫 Directory，
        // 短标签与工具名逐字相同就说明这里根本没翻译。
        else if (value === name) rawNames.push(`${name} → ${value}`);
      }
      expect(missing, `${locale}.json 缺工具行短标签`).toEqual([]);
      expect(rawNames, `${locale}.json 的行短标签退回了裸英文工具名`).toEqual([]);
    });
  }

  it('语言包里没有对不上任何工具的孤儿文案', () => {
    const tool = loadLocale('zh').tool ?? {};
    const known = new Set([
      ...LABELED_TOOL_NAMES.map((n) => TOOL_LABEL_ALIASES[n] ?? n),
      ...ACTION_LABEL_KEYS,
      '_fallback',
      '_plugin',
    ]);
    const orphans = Object.entries(tool)
      .filter(([key, value]) => (value as any)?.running && !known.has(key))
      .map(([key]) => `tool.${key}`);
    expect(orphans, '这些文案键匹配不到任何工具，会永远渲染不出来').toEqual([]);
  });

  it('源码里的工具要么已登记文案，要么显式豁免', () => {
    const registered = scanRegisteredToolNames();
    const labeled = new Set([...LABELED_TOOL_NAMES, ...Object.keys(TOOL_LABEL_ALIASES)]);
    const unregistered = [...registered]
      .filter((name) => !labeled.has(name) && !UNLABELED_TOOL_NAMES.has(name))
      .sort();
    expect(
      unregistered,
      '新工具需要补 tool.<name>.{running,done,failed}，或加进 UNLABELED_TOOL_NAMES 说明它不进进程区',
    ).toEqual([]);
  });

  it('渲染侧的内置名单跟已登记工具对得上', () => {
    // 内置插件（media / beautify / office）是随 Lingxi 分发的，运行时工具名带
    // pluginId 前缀，所以它们是内置工具而不是第三方插件。
    const expected = new Set([
      ...LABELED_TOOL_NAMES.filter((n) => !BUNDLED_PLUGIN_TOOL_NAMES.has(n)),
      ...BUNDLED_PLUGIN_TOOL_NAMES,
      ...Object.keys(RUNTIME_ALIASES),
    ]);
    expect([...BUILTIN_TOOL_NAMES].sort()).toEqual([...expected].sort());
  });

  it('内置工具走通用兜底，第三方插件与 MCP 工具走插件兜底', () => {
    expect(isExternalTool('check_pending_tasks')).toBe(false);
    expect(isExternalTool('web_search')).toBe(false);
    // 内置插件带 pluginId 前缀，仍是内置工具
    expect(isExternalTool('beautify_create-cover')).toBe(false);
    expect(isExternalTool('media_generate-image')).toBe(false);
    expect(isExternalTool('mcp_search_issues')).toBe(true);
    // 第三方插件里叫 read 的工具不能撞上内置 read 的文案
    expect(isExternalTool('acme_read')).toBe(true);
  });

  it('工具行主标签永远取文案词，不返回工具本名', () => {
    // 报告工具里没有对应短标签的冷门工具、插件工具与 MCP 工具
    const label = (name: string, options?: { skill?: boolean; todoTitle?: string }) => {
      const previous = (globalThis as any).window;
      (globalThis as any).window = { t: (key: string) => loadKey(loadLocale('zh'), key) ?? key };
      try {
        return activityLabel(name, options);
      } finally {
        (globalThis as any).window = previous;
      }
    };

    expect(label('search_memory')).toBe('回想');
    expect(label('knowledge_search')).toBe('查资料');
    expect(label('channel_reply')).toBe('回频道');
    expect(label('browser')).toBe('开网页');
    expect(label('computer')).toBe('操控电脑');
    expect(label('materialize')).toBe('落盘');
    expect(label('subagent_reply')).toBe('转达子代理');
    expect(label('create_artifact')).toBe('做卡片');
    expect(label('dm')).toBe('私信');
    // 别名工具走别名键，不各自要一条文案
    expect(label('exec_command')).toBe('Bash');
    expect(label('write_stdin')).toBe('Bash');
    // 技能形态与清单面板标题优先于工具名
    expect(label('read', { skill: true })).toBe('技能');
    expect(label('todo_write', { todoTitle: '任务' })).toBe('任务');

    // 第三方插件 / MCP 工具名不可预知：主标签用统一家族词「扩展」，原工具名不进主标签
    for (const external of ['mcp_deep-search', 'mcp_memory_search', 'acme_read', 'some_plugin_tool']) {
      const text = label(external);
      expect(text).toBe('扩展');
      expect(text).not.toBe(external);
    }
    // 没登记短标签的一方工具落通用词；带下划线又不在内置名单里的（=插件/MCP 形态）
    // 落家族词。两条路都不返回工具本名。
    expect(label('brandnewtool')).toBe('工具');
    expect(label('brand_new_tool')).toBe('扩展');

    // 标签链上的每一档都不能等于工具本名（英文界面最容易踩：labels.tool ≠ "Tool" 以外还要求逐工具翻译）
    const en = loadLocale('en').messageActivity.labels;
    expect(en.tool).toBeTypeOf('string');
    expect(en.tool).not.toBe('');
  });

  it('session 按 action 分档，send/create 不用查看那套说法', () => {
    // read/list 没有专属档位，落回 tool.session
    expect(SESSION_ACTION_LABEL_KEYS.read).toBeUndefined();
    expect(SESSION_ACTION_LABEL_KEYS.list).toBeUndefined();
    expect(SESSION_ACTION_LABEL_KEYS.send).toBe('session_send');
    expect(SESSION_ACTION_LABEL_KEYS.create).toBe('session_create');

    // send/create 那一刻只是拟了草稿卡，文案不能宣布消息已经发出去
    const zh = loadLocale('zh').tool;
    expect(zh.session_send.done).toContain('等你确认');
    expect(zh.session_create.done).toContain('等你确认');
  });

  it('session 工具的目标会话靠 sessionId 查，查不到就不猜', () => {
    const state = {
      sessions: [{ sessionId: 'sess_abc', title: '项目讨论', agentName: '小花' }],
      sessionLocatorsById: { sess_abc: { path: '/agents/hanako/sessions/abc.jsonl' } },
    };
    expect(sessionToolTargetName(state, { action: 'read', sessionId: 'sess_abc' })).toBe('小花 · 项目讨论');
    expect(sessionToolTargetPath(state, { action: 'read', sessionId: 'sess_abc' }))
      .toBe('/agents/hanako/sessions/abc.jsonl');
    // 已归档 / 不在列表里的会话查不到，退回 null 让调用方显示 id 短尾
    expect(sessionToolTargetName(state, { action: 'read', sessionId: 'sess_gone' })).toBeNull();
    expect(sessionToolTargetPath(state, { action: 'read', sessionId: 'sess_gone' })).toBeNull();
    // create 的目标会话还不存在，只给出要派给谁，也没有可跳转的路径
    expect(sessionToolTargetName(state, { action: 'create', agent: '小马' })).toBe('小马');
    expect(sessionToolTargetPath(state, { action: 'create', agent: '小马' })).toBeNull();
  });

  it('失败的工具调用取 failed 相位', () => {
    expect(phaseForStatus('running')).toBe('running');
    expect(phaseForStatus('failed')).toBe('failed');
    expect(phaseForStatus('succeeded')).toBe('done');
    expect(phaseForStatus('unknown')).toBe('done');
  });

  it('插件工具文案带 pluginId 前缀，无前缀键匹配不到任何调用', () => {
    const tool = loadLocale('zh').tool ?? {};
    const bareNames = [
      'generate-image', 'generate-video', 'create-cover',
      'apply-cover-candidate', 'get-cover-style-guide', 'list-capabilities',
    ];
    const stale = bareNames.filter((n) => tool[n]);
    expect(stale, '插件工具运行时名为 `<pluginId>_<tool>`，无前缀键是死键').toEqual([]);
  });
});
