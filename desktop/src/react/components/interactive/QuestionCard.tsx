import { useState } from 'react';
import { AskUserForm } from '../input/AskUserForm';
import type { AskUserQuestion, AskUserValues } from '../input/ask-user-schema';
export interface QuestionRequest { id: string; sessionId: string; runId: string; questions: AskUserQuestion[]; deadlineUnixMs: number }
export function QuestionCard({ request, busy, answer }: { request: QuestionRequest; busy: boolean; answer: (action: 'confirmed' | 'dismissed', values: AskUserValues) => Promise<void> }) {
  const [values, setValues] = useState<AskUserValues>({});
  const [missing, setMissing] = useState<ReadonlySet<string>>(new Set());
  const submit = () => {
    const absent = request.questions.filter(q => q.required && !(Array.isArray(values[q.key]) ? values[q.key].length : String(values[q.key] || '').trim().length)).map(q => q.key);
    setMissing(new Set(absent));
    if (!absent.length) void answer('confirmed', values);
  };
  return <section aria-label="批量提问" data-question-id={request.id}>
    <h3>需要你的选择</h3>
    <AskUserForm form={{ questions: request.questions, unsupported: [] }} values={values} busy={busy} missingKeys={missing} onChange={(key, value) => setValues(previous => ({ ...previous, [key]: value }))} />
    {missing.size > 0 && <p role="alert">请先回答必填问题</p>}
    <button disabled={busy} onClick={submit}>提交答案</button>
    <button disabled={busy} onClick={() => void answer('dismissed', {})}>暂不回答</button>
  </section>;
}
