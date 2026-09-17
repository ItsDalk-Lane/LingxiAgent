import type { SessionConfirmationBlock } from '../../stores/chat-types';

/**
 * Reading an ask_user confirmation block into something renderable.
 *
 * The server-side tool has already validated the payload; this reader still
 * normalizes defensively, because a block can also arrive from a replayed
 * session history written by an older build. A question that cannot be
 * rendered faithfully goes to `unsupported` (and blocks submission), mirroring
 * the elicitation form's honesty rule — never half-render a question and send
 * a fabricated answer back.
 */

export type AskUserQuestionType = 'single' | 'multi' | 'text';

export interface AskUserOption {
  value: string;
  label: string;
  description?: string;
}

export interface AskUserQuestion {
  key: string;
  question: string;
  type: AskUserQuestionType;
  options: AskUserOption[];
  recommended: string[];
  required: boolean;
}

export interface AskUserFormModel {
  questions: AskUserQuestion[];
  /** Keys of questions that could not be faithfully rendered. */
  unsupported: string[];
}

/** key → selected option value (single), selected values (multi), or text. */
export type AskUserValues = Record<string, string | string[]>;

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === 'object' && !Array.isArray(value);
}

function readOption(raw: unknown): AskUserOption | null {
  if (!isPlainObject(raw)) return null;
  const value = typeof raw.value === 'string' && raw.value.trim() ? raw.value.trim() : null;
  if (!value) return null;
  const label = typeof raw.label === 'string' && raw.label.trim() ? raw.label.trim() : value;
  const description = typeof raw.description === 'string' && raw.description.trim()
    ? raw.description.trim()
    : undefined;
  return description ? { value, label, description } : { value, label };
}

function readQuestion(raw: unknown, index: number): { question?: AskUserQuestion; unsupported?: string } {
  const fallbackKey = `q${index + 1}`;
  if (!isPlainObject(raw)) return { unsupported: fallbackKey };
  const question = typeof raw.question === 'string' && raw.question.trim() ? raw.question.trim() : null;
  if (!question) return { unsupported: fallbackKey };
  const key = typeof raw.key === 'string' && raw.key.trim() ? raw.key.trim() : fallbackKey;
  const type = raw.type === 'single' || raw.type === 'multi' || raw.type === 'text' ? raw.type : null;
  if (!type) return { unsupported: key };
  const required = raw.required !== false;

  if (type === 'text') {
    return { question: { key, question, type, options: [], recommended: [], required } };
  }

  const rawOptions = Array.isArray(raw.options) ? raw.options : [];
  const options: AskUserOption[] = [];
  for (const entry of rawOptions) {
    const option = readOption(entry);
    if (!option) return { unsupported: key };
    options.push(option);
  }
  if (options.length === 0) return { unsupported: key };
  const known = new Set(options.map((option) => option.value));
  const recommended = (Array.isArray(raw.recommended) ? raw.recommended : [])
    .filter((item): item is string => typeof item === 'string' && known.has(item));
  return { question: { key, question, type, options, recommended, required } };
}

export function readAskUserForm(block: SessionConfirmationBlock): AskUserFormModel | null {
  if (block.kind !== 'ask_user') return null;
  const raw = block.payload?.questions;
  if (!Array.isArray(raw)) return { questions: [], unsupported: [] };
  const questions: AskUserQuestion[] = [];
  const unsupported: string[] = [];
  const seenKeys = new Set<string>();
  for (const [index, entry] of raw.entries()) {
    const result = readQuestion(entry, index);
    if (result.unsupported !== undefined) {
      unsupported.push(result.unsupported);
      continue;
    }
    const question = result.question as AskUserQuestion;
    if (seenKeys.has(question.key)) {
      unsupported.push(question.key);
      continue;
    }
    seenKeys.add(question.key);
    questions.push(question);
  }
  return { questions, unsupported };
}

/** Recommended options preselect themselves; everything else starts blank. */
export function initialAskUserValues(questions: AskUserQuestion[]): AskUserValues {
  const values: AskUserValues = {};
  for (const q of questions) {
    if (q.type === 'text') values[q.key] = '';
    else if (q.type === 'multi') values[q.key] = [...q.recommended];
    else values[q.key] = q.recommended[0] || '';
  }
  return values;
}

/** Required questions with no usable answer yet. */
export function missingRequiredAskUserQuestions(
  questions: AskUserQuestion[],
  values: AskUserValues,
): AskUserQuestion[] {
  return questions.filter((q) => {
    if (!q.required) return false;
    const raw = values[q.key];
    if (q.type === 'multi') return !Array.isArray(raw) || raw.length === 0;
    return typeof raw !== 'string' || raw.trim() === '';
  });
}

/** Collect the answer map the server tool maps back to labels. */
export function collectAskUserAnswers(
  questions: AskUserQuestion[],
  values: AskUserValues,
): Record<string, unknown> {
  const result: Record<string, unknown> = {};
  for (const q of questions) {
    const raw = values[q.key];
    if (q.type === 'multi') {
      const picked = Array.isArray(raw) ? raw.filter((v): v is string => typeof v === 'string' && !!v) : [];
      if (picked.length > 0) result[q.key] = picked;
      continue;
    }
    const text = typeof raw === 'string' ? raw.trim() : '';
    if (text) result[q.key] = text;
  }
  return result;
}
