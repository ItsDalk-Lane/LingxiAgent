/**
 * R08-A01 前端投影验收：旧 Electron 客户端「不用猜新状态」。
 *
 * 夹具 r08-a01-history.json 是 Rust 服务真实运行的原始 HTTP 响应体
 * （GET /lingxi/v1/sessions/a01/history?all=1），捕获自
 * artifacts/rust-tauri/R08/a01-a02-suite-06/capture/a01-history-raw.json
 * （r08_t01_a01_a02_ws 套件、golden 绑定运行的同一次会话：read → write →
 * edit → exec_command 四工具链 + 最终回复 A01-FINAL-ANSWER）。
 *
 * 本测试把这份真实历史原样喂给生产投影器 projectRustHistoryItems，
 * 断言旧界面拿到的一切状态（工具卡片、最终回复、回合终态）都来自
 * Rust 投影结果本身，前端不做任何猜测或补齐。
 */
import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { projectRustHistoryItems } from '../../services/rust-chat-history';

const fixture = JSON.parse(
  readFileSync(fileURLToPath(new URL('./fixtures/r08-a01-history.json', import.meta.url)), 'utf8'),
) as { schemaVersion: string; sessionId: string; items: unknown[] };

describe('R08-A01 真实历史投影：四工具链会话在旧界面的呈现完全由 Rust 单源决定', () => {
  const items = projectRustHistoryItems(fixture.items, fixture.sessionId);

  it('夹具是真实捕获的 canonical-history.v1 响应', () => {
    expect(fixture.schemaVersion).toBe('lingxi.canonical-history.v1');
    expect(fixture.sessionId).toBe('a01');
    expect(fixture.items.length).toBe(7);
  });

  it('用户消息一条，原文逐字保留', () => {
    const users = items.filter((v) => v.type === 'message' && v.data.role === 'user');
    expect(users).toHaveLength(1);
    if (users[0].type !== 'message') throw new Error('unreachable');
    expect(users[0].data.text).toContain('R08-A01-TOOLCHAIN');
    expect(typeof users[0].data.timestamp).toBe('number');
  });

  it('恰好一个助手回合，终态 completed、结局不是 missing_final_answer', () => {
    const assistants = items.filter((v) => v.type === 'message' && v.data.role === 'assistant');
    expect(assistants).toHaveLength(1);
    if (assistants[0].type !== 'message') throw new Error('unreachable');
    expect(assistants[0].data.id).toMatch(/^native-run:a01:/);
    const projection = assistants[0].data.turnProjection;
    expect(projection?.status).toBe('completed');
    expect(projection?.outcome).not.toBe('completed_without_user_output');
    expect(projection?.answerBlockIds).toHaveLength(1);
  });

  it('四张工具卡片：命名目标、成功状态与顺序全部来自历史投影', () => {
    const assistant = items.find((v) => v.type === 'message' && v.data.role === 'assistant');
    if (assistant?.type !== 'message') throw new Error('unreachable');
    const toolGroups = (assistant.data.blocks ?? []).filter((b) => b.type === 'tool_group');
    expect(toolGroups).toHaveLength(4);
    const tools = toolGroups.flatMap((g) => (g.type === 'tool_group' ? g.tools : []));
    expect(tools.map((t) => t.name)).toEqual([
      'tool:first-party:read',
      'tool:first-party:write',
      'tool:first-party:edit',
      'tool:first-party:exec_command',
    ]);
    for (const tool of tools) {
      expect(tool.status).toBe('succeeded');
      expect(tool.success).toBe(true);
      expect(tool.done).toBe(true);
      expect(tool.id).toMatch(/-tc000\d$/);
      // 持久历史不含 argsSummary（只有 argsDigest，见夹具断言）——旧界面
      // 在重开历史时不许编造参数摘要，钉死该缺席而不是补齐。
      expect(tool.args).toBeUndefined();
    }
    // 夹具层：持久历史以 argsDigest（sha256 + 规范化算法标识）作参数证据。
    const historyTools = (fixture.items as Array<Record<string, unknown>>).filter(
      (v) => v.kind === 'tool_call',
    );
    for (const t of historyTools) {
      const digest = t.argsDigest as Record<string, unknown>;
      expect(digest.algorithm).toBe('sha256');
      expect(digest.canonicalization).toBe('lingxi-canonical-json-v1');
      expect(String(digest.hex)).toMatch(/^[0-9a-f]{64}$/);
    }
    // 无任何伪造终态/失败状态块。
    expect(assistant.data.blocks?.some((b) => b.type === 'turn_status')).toBe(false);
  });

  it('最终回复文本逐字来自 final_message，进入答案区', () => {
    const assistant = items.find((v) => v.type === 'message' && v.data.role === 'assistant');
    if (assistant?.type !== 'message') throw new Error('unreachable');
    const answer = (assistant.data.blocks ?? []).find(
      (b) => b.type === 'text' && b.surfaceRole === 'answer',
    );
    expect(answer).toBeDefined();
    if (answer?.type !== 'text') throw new Error('unreachable');
    expect(answer.source).toContain('A01-FINAL-ANSWER');
    expect(answer.lifecycle).toBe('sealed');
  });
});
