// @vitest-environment jsdom
/**
 * 运行状态行取词对账。
 *
 * 状态行文案只有两个取值点：工具名映射（`RUNNING_STATUS_BY_TOOL`）与两个非工具态
 * （thinking / writing）。语言包里的每个 `chat.running.*` 叶子都必须被其中一条覆盖，
 * 否则就是永远显示不出来的孤儿文案；反过来，映射到不存在的键会静默显示成
 * `chat.running.xxx` 原文。
 */
import '@testing-library/jest-dom/vitest';
import fs from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import {
  RUNNING_STATUS_BY_TOOL,
  runningStatusKey,
  type RunningStatusKey,
} from '../../utils/tool-label';
import { selectRunningActivity } from '../../components/chat/RunningStatusLine';
import { formatRunningDuration } from '../../utils/format-duration';
import type { ContentBlock, ToolCall } from '../../stores/chat-types';

const locales = ['en', 'zh', 'zh-TW', 'ja', 'ko'];
/** 状态行当前真正取到的键：固定文案 + 未知工具兜底（其余是保留文案，见 ALLOWED）。 */
const PRODUCED: RunningStatusKey[] = ['working', 'tool'];
/**
 * 语言包里允许存在但当前不显示的键。
 *
 * 状态行在 2026-09 改成固定的「<助手名>正在工作中」之后，逐步文案（thinking /
 * reading / searching / …）与 selectRunningActivity 的分类保留下来，方便按需切回
 * 分步口吻；这里显式登记，其他未登记的键仍按孤儿处理（防止语言包越堆越乱）。
 */
const ALLOWED: RunningStatusKey[] = [...PRODUCED, ...Object.values(RUNNING_STATUS_BY_TOOL), 'thinking', 'writing'];

/** 与语言包同形的模板翻译（避免为了断言再拉整个 i18n 运行时）。 */
function durationTranslate(templates: Record<string, string>) {
  return (key: string, vars: Record<string, string | number>) => {
    const template = templates[key];
    if (!template) throw new Error(`unknown duration key: ${key}`);
    return template.replace(/\{(\w+)\}/g, (_, name: string) => String(vars[name] ?? ''));
  };
}

const EN_DURATION = {
  'duration.seconds': '{seconds}s',
  'duration.minutes': '{minutes}m {seconds}s',
  'duration.hours': '{hours}h {minutes}m {seconds}s',
};
const ZH_DURATION = {
  'duration.seconds': '{seconds}秒',
  'duration.minutes': '{minutes}分{seconds}秒',
  'duration.hours': '{hours}小时{minutes}分{seconds}秒',
};

function runningStrings(locale: string): Record<string, unknown> {
  const file = path.join(process.cwd(), 'desktop/src/locales', `${locale}.json`);
  return JSON.parse(fs.readFileSync(file, 'utf8')).chat.running;
}

describe('运行状态行取词', () => {
  it('按操作类别归一：内置工具不落 tool 兜底', () => {
    expect(runningStatusKey('read')).toBe('reading');
    expect(runningStatusKey('materialize')).toBe('reading');
    expect(runningStatusKey('grep')).toBe('searching');
    expect(runningStatusKey('ls')).toBe('searching');
    expect(runningStatusKey('edit')).toBe('editing');
    expect(runningStatusKey('write')).toBe('editing');
    expect(runningStatusKey('exec_command')).toBe('command');
    expect(runningStatusKey('web_fetch')).toBe('web');
    expect(runningStatusKey('recall_experience')).toBe('memory');
    expect(runningStatusKey('knowledge_search')).toBe('knowledge');
    expect(runningStatusKey('knowledge_research_round')).toBe('knowledge');
  });

  it('第三方工具不猜类别：插件 read 与 MCP 工具都落 tool', () => {
    expect(runningStatusKey('myplugin_read')).toBe('tool');
    expect(runningStatusKey('mcp_search')).toBe('tool');
    expect(runningStatusKey('computer')).toBe('tool');
  });

  for (const locale of locales) {
    it(`${locale}.json 的运行状态文案覆盖每个可产生的键，且非空`, () => {
      const running = runningStrings(locale);
      const missing = PRODUCED.filter(key => typeof running[key] !== 'string' || !String(running[key]).trim());
      expect(missing, `${locale}.json 缺 chat.running.*`).toEqual([]);
      // 固定文案要把助手名插进去；工具目录类模板同理。
      expect(running.working).toContain('{name}');
      expect(running.tool).toContain('{name}');
    });

    it(`${locale}.json 没有状态行取不到的孤儿文案`, () => {
      const orphans = Object.keys(runningStrings(locale)).filter(key => !ALLOWED.includes(key as RunningStatusKey));
      expect(orphans, `${locale}.json 孤儿 chat.running.*`).toEqual([]);
    });
  }
});

describe('运行秒表', () => {
  it('不足一分钟只报秒；跨档后秒数补零，避免读数每秒跳宽度', () => {
    const t = durationTranslate(EN_DURATION);
    expect(formatRunningDuration(0, t)).toBe('0s');
    expect(formatRunningDuration(7_400, t)).toBe('7s');
    expect(formatRunningDuration(59_999, t)).toBe('59s');
    expect(formatRunningDuration(60_000, t)).toBe('1m 00s');
    expect(formatRunningDuration(65_400, t)).toBe('1m 05s');
    expect(formatRunningDuration(3_600_000, t)).toBe('1h 00m 00s');
    expect(formatRunningDuration(3_725_000, t)).toBe('1h 02m 05s');
  });

  it('负数时钟偏差 clamp 到 0', () => {
    expect(formatRunningDuration(-4_000, durationTranslate(EN_DURATION))).toBe('0s');
  });

  it('中文档走「1分05秒」而不是英文读数', () => {
    const t = durationTranslate(ZH_DURATION);
    expect(formatRunningDuration(65_400, t)).toBe('1分05秒');
    expect(formatRunningDuration(7_400, t)).toBe('7秒');
  });

  for (const locale of locales) {
    it(`${locale}.json 提供三档耗时模板且带全部占位符`, () => {
      const duration = JSON.parse(
        fs.readFileSync(path.join(process.cwd(), 'desktop/src/locales', `${locale}.json`), 'utf8'),
      ).duration;
      expect(duration.seconds).toContain('{seconds}');
      expect(duration.minutes).toContain('{minutes}');
      expect(duration.minutes).toContain('{seconds}');
      expect(duration.hours).toContain('{hours}');
      expect(duration.hours).toContain('{minutes}');
      expect(duration.hours).toContain('{seconds}');
    });
  }
});

const tool = (name: string, extra: Partial<ToolCall> = {}): ToolCall => ({
  id: name, name, args: {}, done: false, success: false, status: 'running', ...extra,
});

describe('当前动作投影', () => {
  it('取最后一个在跑的工具，忽略已完成的与不进进程区的', () => {
    const blocks: ContentBlock[] = [
      { type: 'tool_group', collapsed: false, tools: [
        tool('edit', { done: true, success: true, status: 'succeeded' }),
        tool('stage_files', { name: 'stage_files' }),
        tool('grep'),
      ] },
    ];
    expect(selectRunningActivity(blocks)).toEqual({ key: 'searching' });
  });

  it('在途工具只有 done:false（没有 status）也算在跑', () => {
    const legacy: ToolCall = { id: 't', name: 'read', args: {}, done: false, success: false };
    const blocks = [{ type: 'tool_group', collapsed: false, tools: [legacy] }] as ContentBlock[];
    expect(selectRunningActivity(blocks)).toEqual({ key: 'reading' });
  });

  it('未知工具带回工具名，供 {name} 模板使用', () => {
    const blocks = [{ type: 'tool_group', collapsed: false, tools: [tool('myplugin_scan')] }] as ContentBlock[];
    expect(selectRunningActivity(blocks)).toEqual({ key: 'tool', tool: 'myplugin_scan' });
  });

  it('没有在跑的工具时看未封口思考，再看流式正文；两者都没有则不显示', () => {
    const settled = [{ type: 'tool_group', collapsed: false, tools: [tool('read', { done: true, success: true, status: 'succeeded' })] }] as ContentBlock[];
    expect(selectRunningActivity([...settled, { type: 'thinking', content: '想', sealed: false }])).toEqual({ key: 'thinking' });
    expect(selectRunningActivity([...settled, { type: 'thinking', content: '想完了', sealed: true }])).toBeNull();
    expect(selectRunningActivity([...settled, { type: 'text', source: '正文' }])).toEqual({ key: 'writing' });
    expect(selectRunningActivity([])).toBeNull();
    expect(selectRunningActivity(null)).toBeNull();
  });
});
