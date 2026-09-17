import { describe, expect, it } from 'vitest';
import {
  collectAskUserAnswers,
  initialAskUserValues,
  missingRequiredAskUserQuestions,
  readAskUserForm,
} from '../../components/input/ask-user-schema';
import type { SessionConfirmationBlock } from '../../stores/chat-types';

function block(questions: unknown, kind = 'ask_user'): SessionConfirmationBlock {
  return {
    type: 'session_confirmation',
    confirmId: 'c1',
    kind,
    surface: 'input',
    status: 'pending',
    title: '问题',
    payload: { questions },
  };
}

const QUESTIONS = [
  {
    key: 'approach',
    question: '用哪种方案？',
    type: 'single',
    options: [
      { value: 'a', label: '方案 A', description: '稳妥但慢' },
      { value: 'b', label: '方案 B' },
    ],
    recommended: ['a'],
    required: true,
  },
  {
    key: 'extras',
    question: '还要哪些？',
    type: 'multi',
    options: [
      { value: 'x', label: '额外 X' },
      { value: 'y', label: '额外 Y' },
    ],
    recommended: ['x'],
    required: false,
  },
  { key: 'note', question: '补充', type: 'text', options: [], recommended: [], required: false },
];

describe('ask-user schema', () => {
  it('非 ask_user 块返回 null', () => {
    expect(readAskUserForm(block([], 'mcp_elicitation'))).toBeNull();
  });

  it('规范化读取：推荐项过滤掉未知值，缺省 key 落 q1/q2', () => {
    const form = readAskUserForm(block([
      { question: '选一个', type: 'single', options: [{ value: 'a' }, { value: 'b' }], recommended: ['a', 'ghost'] },
      { question: '说说', type: 'text' },
    ]))!;
    expect(form.unsupported).toEqual([]);
    expect(form.questions.map(q => q.key)).toEqual(['q1', 'q2']);
    expect(form.questions[0].recommended).toEqual(['a']);
    expect(form.questions[0].options[0]).toEqual({ value: 'a', label: 'a' });
    expect(form.questions[1].required).toBe(true);
  });

  it('渲染不了的问题进 unsupported（不半渲染）', () => {
    const form = readAskUserForm(block([
      'not-an-object',
      { question: '', type: 'text' },
      { question: '坏类型', type: 'ranking' },
      { question: '没选项', type: 'single', options: [] },
      ...QUESTIONS,
    ]))!;
    expect(form.unsupported).toEqual(['q1', 'q2', 'q3', 'q4']);
    expect(form.questions).toHaveLength(3);
  });

  it('重复 key 的后到者进 unsupported', () => {
    const form = readAskUserForm(block([
      { key: 'dup', question: '第一问', type: 'text' },
      { key: 'dup', question: '第二问', type: 'text' },
    ]))!;
    expect(form.questions).toHaveLength(1);
    expect(form.unsupported).toEqual(['dup']);
  });

  it('初始值：推荐项预选，其余留空', () => {
    const values = initialAskUserValues(readAskUserForm(block(QUESTIONS))!.questions);
    expect(values).toEqual({ approach: 'a', extras: ['x'], note: '' });
  });

  it('必填判定：单选要选一个，多选至少一个，文本非空', () => {
    const questions = readAskUserForm(block(QUESTIONS))!.questions;
    // 初始值里 approach/extras 已被推荐项填上 → 不缺
    expect(missingRequiredAskUserQuestions(questions, initialAskUserValues(questions))).toEqual([]);
    // 清空单选 → 缺 approach
    const missing = missingRequiredAskUserQuestions(questions, { approach: '', extras: [], note: '' });
    expect(missing.map(q => q.key)).toEqual(['approach']);
  });

  it('收集答案：空的可选项不发送，多选滤掉空值', () => {
    const questions = readAskUserForm(block(QUESTIONS))!.questions;
    const answers = collectAskUserAnswers(questions, { approach: 'b', extras: ['y', ''], note: '  ' });
    expect(answers).toEqual({ approach: 'b', extras: ['y'] });
  });
});
