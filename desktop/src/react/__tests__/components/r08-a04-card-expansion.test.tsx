// @vitest-environment jsdom

/**
 * R08-A04「卡片展开不退化」组件验收：
 * 前置 = 只有工具结果没有最终回复的会话（服务套件 r08_a04_no_final_ws 的
 * 真实历史捕获，原样拷贝为夹具）；操作 = 展开汇总区再点击任意工具卡；
 * 通过 = 正常展开，错误/思考/产物内容可查看。
 *
 * 全链路只走真实路径：canonical 历史夹具 → projectRustHistoryItems →
 * ChatTranscript + ProcessFoldBlock + ToolGroupBlock + ThinkingBlock。
 * 不手搭投影结果，不mock任何产品组件。
 */
import '@testing-library/jest-dom/vitest';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ChatTranscript } from '../../components/chat/ChatTranscript';
import { projectRustHistoryItems } from '../../services/rust-chat-history';
import { useStore } from '../../stores';
import type { ChatListItem, ChatMessage, ContentBlock } from '../../stores/chat-types';

const MISSING_ERROR = 'Could not read file: a04-missing.txt. Error code: ENOENT.';
const NO_FINAL_REASON = 'completed.no_final.tool_partial_failure';
const sessionPath = '/session/r08-a04.jsonl';

interface HistoryFixture {
  schemaVersion: string;
  sessionId: string;
  revision: string;
  items: Array<Record<string, unknown>>;
}
const fixture = JSON.parse(readFileSync(
  path.join(process.cwd(), 'desktop/src/react/__tests__/services/fixtures/r08-a04-history.json'),
  'utf8',
)) as HistoryFixture;

const projected = projectRustHistoryItems(fixture.items as unknown[], fixture.sessionId);
const assistantItem = projected.find(
  (item) => item.type === 'message' && item.data.role === 'assistant',
);
if (!assistantItem || assistantItem.type !== 'message') throw new Error('夹具投影缺少助手轮');
const assistant = assistantItem.data as ChatMessage;
const blocks = assistant.blocks ?? [];
const toolGroups = blocks.filter((block): block is Extract<ContentBlock, { type: 'tool_group' }> => block.type === 'tool_group');
const thinkingBlocks = blocks.filter((block) => block.type === 'thinking');
const turnStatusBlocks = blocks.filter((block): block is Extract<ContentBlock, { type: 'turn_status' }> => block.type === 'turn_status');

function t(key: string, vars?: Record<string, string | number>): string {
  const table: Record<string, string> = {
    'messageActivity.labels.thinking': '思考',
    'toolGroup.count': '{n} 个工具',
    'toolGroup.countWithFail': '{total} 个工具（{fail} 个失败）',
    'tool._fallback.done': '小花 忙完了',
    'tool._fallback.running': '小花 忙着',
    'processFold.summary': '✨ {name}忙活了一阵子',
    'processFold.tools': '{n} 个工具',
    'processFold.thinking': '{n} 次思考',
    'processFold.unsuccessful': '{n} 次尝试未成功',
    'chat.turnStatus.missingFinalAnswer': '本轮结束，没有产出最终回复',
    'common.regenerate': '重新生成',
    'common.forkSession': '分支为新会话',
  };
  return (table[key] || key).replace(/\{(\w+)\}/g, (_, name) => String(vars?.[name] ?? ''));
}

function renderTranscript() {
  return render(
    <ChatTranscript items={projected} sessionPath={sessionPath} enableProcessFold />,
  );
}

function foldSummary(): HTMLElement {
  const summary = document.querySelector('button[class*="processFoldSummary"]');
  if (!(summary instanceof HTMLElement)) throw new Error('缺少过程汇总按钮');
  return summary;
}

function toolRow(callId: string): HTMLElement {
  const row = document.querySelector(`[data-tool-call-id="${callId}"]`);
  if (!(row instanceof HTMLElement)) throw new Error(`缺少工具卡 ${callId}`);
  return row;
}

describe('R08-A04 卡片展开不退化', () => {
  beforeEach(() => {
    window.t = t as typeof window.t;
    useStore.setState({
      agents: [],
      agentName: '小花',
      agentYuan: 'lingxi',
      streamingSessions: [],
      selectedIdsBySession: {},
      currentSessionId: null,
      sessions: [],
      sessionLocatorsById: {},
      chatSessions: {
        [sessionPath]: { hasMore: false, loadingMore: false, items: [] },
      },
      currentSessionPath: sessionPath,
      terminalsBySession: {},
    } as never);
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', {
      configurable: true,
      value: vi.fn(),
    });
    // 注意：不 mock requestAnimationFrame —— Collapse 退场动画依赖真实
    // RAF 推进，mock 成同步单次会让收折后的面板永远留在 DOM。
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('夹具即服务捕获：无最终回复、失败工具与诚实终态逐字段可查', () => {
    expect(fixture.schemaVersion).toBe('lingxi.canonical-history.v1');
    expect(fixture.sessionId).toBe('a04');
    expect(fixture.items.map((item) => item.kind)).toEqual([
      'user_message', 'assistant_segment', 'assistant_segment',
      'tool_call', 'tool_call', 'run_terminal',
    ]);
    // 无任何 final_message：本场景的核心前置。
    expect(fixture.items.some((item) => item.kind === 'final_message')).toBe(false);

    const segments = fixture.items.filter((item) => item.kind === 'assistant_segment');
    expect(segments).toHaveLength(2);
    const reasoning = segments.find((item) => item.segmentKind === 'reasoning');
    const narration = segments.find((item) => item.segmentKind === 'text');
    expect(reasoning?.phase).toBe('reasoning');
    expect(reasoning?.complete).toBe(true);
    expect(String(reasoning?.text)).toContain('A04-THINKING-ALOUD');
    // 无 final 的叙述段永远落 unresolved，绝不伪造 final_answer。
    expect(narration?.phase).toBe('unresolved');
    expect(narration?.complete).toBe(true);
    expect(String(narration?.text)).toContain('A04-READING-SEED');

    const tools = fixture.items.filter((item) => item.kind === 'tool_call');
    expect(tools).toHaveLength(2);
    for (const tool of tools) {
      expect(tool.target).toBe('tool:first-party:read');
      expect(String(tool.toolCallId)).toMatch(/^run_[0-9a-f]{16}_\d{6}-tc000[12]$/);
      const digest = tool.argsDigest as Record<string, unknown>;
      expect(digest.algorithm).toBe('sha256');
      expect(String(digest.hex)).toMatch(/^[0-9a-f]{64}$/);
    }
    const okResult = tools[0].result as Record<string, unknown>;
    expect(tools[0].status).toBe('success');
    expect((okResult.content as Array<{ text: string }>)[0].text).toBe('A04-SEED-CONTENT\n');
    expect((okResult.details as { source: { path: string } }).source.path).toBe('a04-seed.txt');
    expect(tools[1].status).toBe('failed');
    const failedError = (tools[1].result as { error: { code: string; message: string } }).error;
    expect(failedError.code).toBe('not_found');
    expect(failedError.message).toBe(MISSING_ERROR);

    const terminal = fixture.items.find((item) => item.kind === 'run_terminal');
    expect(terminal?.status).toBe('completed');
    expect(terminal?.terminalReason).toBe(NO_FINAL_REASON);
    expect(String(terminal?.eventSeq)).toMatch(/^\d+$/);
    // run 归属同源：段/工具/终态指向同一 run。
    const runId = String(terminal?.runId);
    for (const item of [...segments, ...tools]) {
      expect((item.run as { state: string }).state).toBe('known');
      expect((item.run as { runId: string }).runId).toBe(runId);
    }
  });

  it('真实投影：过程块成卡、缺最终回复结果块上答案面', () => {
    expect(projected).toHaveLength(2);
    expect(projected[0].type).toBe('message');
    if (projected[0].type !== 'message') throw new Error('unreachable');
    expect(projected[0].data.role).toBe('user');
    expect(projected[0].data.text).toBe('R08-A04-NO-FINAL');

    const projection = assistant.turnProjection;
    expect(projection?.status).toBe('completed');
    expect(projection?.outcome).toBe('completed_without_user_output');
    expect(projection?.missingFinalAnswerReason).toBe('only_process_blocks');
    expect(projection?.answerBlockIds ?? []).toHaveLength(0);

    expect(thinkingBlocks).toHaveLength(1);
    expect(String((thinkingBlocks[0] as { content: string }).content)).toContain('A04-THINKING-ALOUD');
    const processText = blocks.find((block) => block.type === 'text');
    expect(processText?.surfaceRole).toBe('process');
    expect(String((processText as { source?: string })?.source)).toContain('A04-READING-SEED');

    expect(toolGroups).toHaveLength(2);
    const [okGroup, failedGroup] = toolGroups;
    expect(okGroup.tools[0].name).toBe('tool:first-party:read');
    expect(okGroup.tools[0].status).toBe('succeeded');
    expect(okGroup.tools[0].success).toBe(true);
    expect(okGroup.tools[0].done).toBe(true);
    expect(okGroup.tools[0].error).toBeUndefined();
    expect(failedGroup.tools[0].status).toBe('failed');
    expect(failedGroup.tools[0].success).toBe(false);
    expect(failedGroup.tools[0].error).toBe(MISSING_ERROR);
    expect(String(okGroup.tools[0].id)).toMatch(/-tc0001$/);
    expect(String(failedGroup.tools[0].id)).toMatch(/-tc0002$/);

    expect(turnStatusBlocks).toHaveLength(1);
    expect(turnStatusBlocks[0].status).toBe('missing_final_answer');
    expect(projection?.resultBlockIds).toContain(turnStatusBlocks[0].id);
  });

  it('展开汇总区再逐卡点击：错误/思考/产物全部可查，收折不退化', async () => {
    renderTranscript();

    // 缺最终回复结果块在折叠外立即可见（诚实呈现，不被汇总区吞掉）。
    expect(screen.getByText('本轮结束，没有产出最终回复')).toBeInTheDocument();

    const summary = foldSummary();
    expect(summary).toHaveAttribute('aria-expanded', 'false');
    // 折叠时过程内容不可见。
    expect(screen.queryByText('A04-THINKING-ALOUD')).not.toBeInTheDocument();
    expect(screen.queryByText('A04-READING-SEED')).not.toBeInTheDocument();
    expect(document.querySelector('[data-tool-call-id]')).toBeNull();

    // ── 操作 1：展开汇总区 ──
    fireEvent.click(summary);
    expect(summary).toHaveAttribute('aria-expanded', 'true');

    const [okId, failedId] = [
      String(toolGroups[0].tools[0].id),
      String(toolGroups[1].tools[0].id),
    ];
    const okRow = toolRow(okId);
    const failedRow = toolRow(failedId);
    expect(okRow).toHaveAttribute('data-status', 'succeeded');
    expect(failedRow).toHaveAttribute('data-status', 'failed');
    // 失败工具卡即使未点开，行摘要也如实展示错误。
    expect(within(failedRow).getByText(MISSING_ERROR)).toBeInTheDocument();
    // 过程叙述文本在汇总区内可见。
    expect(screen.getByText('A04-READING-SEED')).toBeInTheDocument();

    // ── 操作 2：点击失败工具卡 → 错误内容可查看 ──
    const failedToggle = within(failedRow).getByRole('button');
    expect(failedToggle).toHaveAttribute('aria-expanded', 'false');
    fireEvent.click(failedToggle);
    expect(failedToggle).toHaveAttribute('aria-expanded', 'true');
    expect(within(failedRow).getAllByText(MISSING_ERROR).length).toBeGreaterThan(0);

    // ── 操作 3：点击成功工具卡 → 正常展开，产物详情面板可查 ──
    const okToggle = within(okRow).getByRole('button');
    expect(okToggle).toHaveAttribute('aria-expanded', 'false');
    fireEvent.click(okToggle);
    expect(okToggle).toHaveAttribute('aria-expanded', 'true');
    expect(within(okRow).queryByText(MISSING_ERROR)).not.toBeInTheDocument();

    // ── 操作 4：点击思考卡 → 思考内容可查看 ──
    const thinking = document.querySelector('[data-thinking]');
    if (!(thinking instanceof HTMLElement)) throw new Error('缺少思考卡');
    const thinkingToggle = within(thinking).getByRole('button');
    expect(thinkingToggle).toHaveAttribute('aria-expanded', 'false');
    fireEvent.click(thinkingToggle);
    expect(thinkingToggle).toHaveAttribute('aria-expanded', 'true');
    expect(within(thinking).getAllByText('A04-THINKING-ALOUD').length).toBeGreaterThan(0);

    // ── 往返收折不退化：再次点击汇总区，过程卡全部收起 ──
    fireEvent.click(summary);
    expect(summary).toHaveAttribute('aria-expanded', 'false');
    // Collapse 退场动画结束后才卸载面板，等它真实完成。
    await waitFor(() => {
      expect(document.querySelector('[data-tool-call-id]')).toBeNull();
      expect(screen.queryByText('A04-THINKING-ALOUD')).not.toBeInTheDocument();
    });
    // 结果块始终在位，不随收折丢失。
    expect(screen.getByText('本轮结束，没有产出最终回复')).toBeInTheDocument();
  });
});
