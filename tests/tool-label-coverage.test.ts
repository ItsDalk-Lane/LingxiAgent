import { describe, expect, it } from 'vitest';
import fs from 'node:fs';
import path from 'node:path';
import {
  ACTIVITY_LABEL_KEYS, BUILTIN_TOOL_NAMES, BUNDLED_PLUGIN_TOOL_NAMES, TOOL_LABEL_ALIASES as RUNTIME_ALIASES,
  SESSION_ACTION_LABEL_KEYS, activityLabel,
  isExternalTool, phaseForStatus, sessionToolTargetName, sessionToolTargetPath,
} from '../desktop/src/react/utils/tool-label';
import { isToolCallHiddenFromProcessUi } from '../desktop/src/react/utils/tool-call-visibility';

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

/**
 * 需要文案的工具名（含插件工具的 `<pluginId>_<tool>` 全名）。
 *
 * 这张表**不再**是"工具全集"的真相源，重建自独立来源（见 toolNameCensus），
 * 只用于历史兼容工具与老 `tool.*` 整句文案的核对。
 */
const LIVE_TOOL_FIXTURE_NAMES = [
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
];

/**
 * 已下线、只可能出现在历史 JSONL 里的工具。
 *
 * 独立于渲染侧登记表：这些名字由 test 显式列出（而不是从 ACTIVITY_LABEL_KEYS
 * 派生），所以"回看旧会话时漏配短标签"能变成红灯。
 * 三个名字都对应真实历史记录形态（present_files 见 server/block-extractors.ts
 * 的 COMPAT 注释）。
 */
const LEGACY_HISTORICAL_TOOL_NAMES = new Set(['create_artifact', 'dm', 'present_files']);

/**
 * 工具行主标签短文案覆盖的工具名。
 *
 * 取运行时的 ACTIVITY_LABEL_KEYS（登记表）加别名键：别名工具（exec_command /
 * write_stdin）在表里指向 terminal，本身不需要独立文案键，但必须有短标签可渲染。
 *
 * 这一组刻意由登记表派生——它守的是"登记表里的每一项在五语言里都真的存在"。
 * "工具全集有没有漏项"是另一件事，由 toolNameCensus() 从独立真相源回答；
 * 两者合起来才是双向对账，只留派生那一半就是自证。
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
const LEGACY_TOOL_NAMES = LEGACY_HISTORICAL_TOOL_NAMES;

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

/** Pi SDK 固定工具（`lib/pi-sdk/index.ts` 转出的 create*Tool 工厂）。 */
const PI_SDK_FIXED_TOOL_NAMES = ['read', 'write', 'edit', 'ls', 'bash'];

/**
 * 内置插件工具全集：**独立真相源**，从 `plugins/<id>/manifest.json` 的 id 与
 * `plugins/<id>/tools/*.ts` 导出的工具名算出运行时名 `<id>_<name>`
 * （命名规则见 core/plugin-manager.ts `_loadTools` 的 `${entry.id}_${mod.name}`），
 * 不读渲染侧的 BUNDLED_PLUGIN_TOOL_NAMES。
 *
 * `jimeng-cli` 这类没有 `tools/` 目录的插件是 provider 而不是工具插件，自然不产出名字。
 */
function scanBundledPluginToolNames(): { names: Set<string>; plugins: Map<string, string[]> } {
  const names = new Set<string>();
  const plugins = new Map<string, string[]>();
  const root = path.join(process.cwd(), 'plugins');
  for (const entry of fs.readdirSync(root, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const manifestPath = path.join(root, entry.name, 'manifest.json');
    const toolsDir = path.join(root, entry.name, 'tools');
    if (!fs.existsSync(manifestPath) || !fs.existsSync(toolsDir)) continue;
    const pluginId = JSON.parse(fs.readFileSync(manifestPath, 'utf8')).id;
    expect(pluginId, `${entry.name}/manifest.json 的 id 决定运行时前缀`).toBeTypeOf('string');
    const toolNames: string[] = [];
    for (const file of fs.readdirSync(toolsDir).filter((f) => f.endsWith('.ts'))) {
      const text = fs.readFileSync(path.join(toolsDir, file), 'utf8');
      // 只认真正会被 _loadTools 发布的模块：必须同时导出 name / description / execute。
      if (!/export\s+(?:async\s+)?function\s+execute\b/.test(text) || !/export\s+const\s+description\b/.test(text)) continue;
      const exported = /export\s+const\s+name\s*=\s*"([^"]+)"/.exec(text);
      if (!exported) continue;
      const runtimeName = `${pluginId}_${exported[1]}`;
      toolNames.push(runtimeName);
      names.add(runtimeName);
    }
    plugins.set(pluginId, toolNames.sort());
  }
  return { names, plugins };
}

/**
 * 真实工具全集：由独立来源构造，**不含** ACTIVITY_LABEL_KEYS / BUILTIN_TOOL_NAMES
 * 派生出的任何集合。历史兼容工具单独标注，因为它们不在当前注册面里。
 *
 * 返回的是"会出现在会话时间线里的工具名"，对账据此判断每个名字要么有短标签、
 * 要么显式豁免（豁免名单见 UNLABELED_TOOL_NAMES / ACTIVITY_LABEL_EXEMPT_TOOL_NAMES）。
 */
function toolNameCensus() {
  const registered = scanRegisteredToolNames();
  const bundled = scanBundledPluginToolNames();
  const runtime = new Set<string>([...registered, ...bundled.names, ...PI_SDK_FIXED_TOOL_NAMES]);
  const declaredOnly = LIVE_TOOL_FIXTURE_NAMES.filter((name) => !runtime.has(name));
  return { runtime, registered, bundled, declaredOnly };
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
      for (const name of [...LIVE_TOOL_FIXTURE_NAMES, ...LEGACY_HISTORICAL_TOOL_NAMES, ...ACTION_LABEL_KEYS]) {
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
      ...LIVE_TOOL_FIXTURE_NAMES.map((n) => TOOL_LABEL_ALIASES[n] ?? n),
      // 内置插件工具（含 media_generate-speech 这类只在 plugins/ 源码里定义的工具）
      // 与历史兼容工具都有自己的 tool.* 整句文案，不能因为静态扫描抓不到就被判成孤儿。
      ...[...BUNDLED_PLUGIN_TOOL_NAMES].map((n) => TOOL_LABEL_ALIASES[n] ?? n),
      ...[...BUILTIN_TOOL_NAMES].map((n) => TOOL_LABEL_ALIASES[n] ?? n),
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
    const labeled = new Set([...LIVE_TOOL_FIXTURE_NAMES, ...LEGACY_HISTORICAL_TOOL_NAMES, ...Object.keys(TOOL_LABEL_ALIASES)]);
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
      ...LIVE_TOOL_FIXTURE_NAMES.filter((n) => !BUNDLED_PLUGIN_TOOL_NAMES.has(n)),
      ...LEGACY_HISTORICAL_TOOL_NAMES,
      ...BUNDLED_PLUGIN_TOOL_NAMES,
      ...Object.keys(RUNTIME_ALIASES),
    ]);
    expect([...BUILTIN_TOOL_NAMES].sort()).toEqual([...expected].sort());
  });

  describe('双向 census（期望集合来自独立真相源，不用被测登记表派生）', () => {
    const census = toolNameCensus();

    it('内置插件工具全集由 plugins/*/manifest.json + tools/*.ts 独立算出，并全部在内置名单里', () => {
      // 这条独立于渲染侧名单：新加一个 plugins/<id>/tools/<tool>.ts 而忘了登记内置插件，
      // 这里就红。office_html_to-pdf 这类"名字写错了"的漏配由下一条守。
      expect(census.bundled.plugins.get('office')).toEqual([
        'office_html-to-pdf', 'office_list-capabilities', 'office_read-document',
      ]);
      const pluginTools = [...census.bundled.names].sort();
      expect(pluginTools.length, 'plugins/ 下应能扫出内置插件工具').toBeGreaterThan(0);
      const notBuiltin = pluginTools.filter((name) => !BUNDLED_PLUGIN_TOOL_NAMES.has(name));
      expect(notBuiltin, '这些内置插件工具没进 BUNDLED_PLUGIN_TOOL_NAMES，会被误判成第三方插件').toEqual([]);
      // 反向：内置名单里的插件工具必须真的存在于 plugins/ 源码里（防名单过期）
      const ghost = [...BUNDLED_PLUGIN_TOOL_NAMES].filter((name) => !census.bundled.names.has(name) && !census.runtime.has(name));
      expect(ghost, '内置插件名单里有 plugins/ 源码里找不到的工具名').toEqual([]);
    });

    it('真实工具全集 → 可见性 → 需要短标签的工具，全部命中 ACTIVITY_LABEL_KEYS', () => {
      const needsLabel = [...census.runtime]
        // 可见性判断复用渲染侧唯一入口：卡片承载 / 子代理 / stage_files 这些
        // 根本不进进程区的工具，不该被要求配一个永远用不上的短标签。
        .filter((name) => !isToolCallHiddenFromProcessUi({ name, args: {} }))
        .filter((name) => !UNLABELED_TOOL_NAMES.has(name))
        .filter((name) => !ACTIVITY_LABEL_EXEMPT_TOOL_NAMES.has(name))
        .sort();
      const missing = needsLabel.filter((name) => {
        const key = RUNTIME_ALIASES[name] ?? name;
        return !ACTIVITY_LABEL_KEYS[key];
      });
      expect(
        missing,
        '这些真实工具会渲染成工具行却查不到专属短标签，只会显示通用词；请登记 ACTIVITY_LABEL_KEYS',
      ).toEqual([]);
    });

    it('历史兼容工具即使已下线，仍要有短标签或明确豁免', () => {
      // present_files 会进 ToolGroupBlock（不在 tool-call-visibility 的隐藏名单里），
      // 旧会话回放时必须显示专属短标签而不是泛化的"工具"。
      const missing = [...LEGACY_HISTORICAL_TOOL_NAMES].filter((name) => {
        const key = RUNTIME_ALIASES[name] ?? name;
        return !ACTIVITY_LABEL_KEYS[key] && !ACTIVITY_LABEL_EXEMPT_TOOL_NAMES.has(name);
      });
      expect(missing, '历史兼容工具缺短标签，旧会话回放会退化成通用词').toEqual([]);
      expect(ACTIVITY_LABEL_KEYS.present_files, 'present_files 需要专属短标签键').toBe('present_files');
    });

    it('ACTIVITY_LABEL_KEYS 里不是家族键的项目必须能映射到真实工具 / 别名 / 历史兼容工具 / 明确豁免', () => {
      // 反向 census：登记表里的每一项都必须落进渲染侧真正认得的工具全集
      // （BUILTIN_TOOL_NAMES 由上面几条与本 census 双向核对过），否则就是死键。
      // 典型死键是拼错的工具名：office_html_to-pdf 永远匹配不到 office_html-to-pdf。
      const known = new Set<string>([
        ...BUILTIN_TOOL_NAMES,
        ...Object.keys(RUNTIME_ALIASES),
        ...ACTION_LABEL_KEYS,
        ...UNLABELED_TOOL_NAMES,
      ]);
      const orphans = Object.keys(ACTIVITY_LABEL_KEYS)
        .filter((name) => !name.startsWith('_'))
        .filter((name) => !known.has(name))
        .sort();
      expect(
        orphans,
        '这些短标签键映射不到任何真实工具、别名、历史兼容工具或豁免项，是拼错或过期的死键',
      ).toEqual([]);
    });

    it('插件工具的运行时长名写错一位就会被抓出来（office_html_to-pdf 型死键）', () => {
      // 直接对账"运行时名 → 短标签键"，而不是相信登记表自己。
      for (const name of census.bundled.names) {
        expect(ACTIVITY_LABEL_KEYS[name], `${name} 没有专属短标签键（运行时名拼错会落通用兜底）`).toBeDefined();
      }
      // 如果哪天有人把连字符写成下划线，上面的断言会红；这里把反例固定下来。
      expect(ACTIVITY_LABEL_KEYS['office_html_to-pdf']).toBeUndefined();
      expect(ACTIVITY_LABEL_KEYS['office_html-to-pdf']).toBe('office_html-to-pdf');
    });

    it('office 工具按运行时命名规则命中专属短标签，不再落通用兜底', () => {
      // 运行时名来自 plugins/office/tools/*.ts（连字符保留），不是 registry 里的下划线写法。
      const officeTools = census.bundled.plugins.get('office') ?? [];
      expect(officeTools).toContain('office_html-to-pdf');
      for (const name of officeTools) {
        expect(ACTIVITY_LABEL_KEYS[name], `${name} 缺短标签键`).toBe(name);
      }
    });
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
