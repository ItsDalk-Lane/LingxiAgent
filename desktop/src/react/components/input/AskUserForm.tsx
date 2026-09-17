import styles from './InputArea.module.css';
import type { AskUserFormModel, AskUserQuestion, AskUserValues } from './ask-user-schema';

function textWithFallback(key: string, fallback: string) {
  const translated = window.t?.(key);
  return translated && translated !== key ? translated : fallback;
}

interface AskUserFormProps {
  form: AskUserFormModel;
  values: AskUserValues;
  busy: boolean;
  /** Keys of required questions the user has been told about, after a blocked submit. */
  missingKeys: ReadonlySet<string>;
  onChange: (key: string, value: string | string[]) => void;
}

function OptionRow({
  question,
  option,
  checked,
  busy,
  onToggle,
}: {
  question: AskUserQuestion;
  option: { value: string; label: string; description?: string };
  checked: boolean;
  busy: boolean;
  onToggle: (checked: boolean) => void;
}) {
  const recommended = question.recommended.includes(option.value);
  return (
    <label className={styles['ask-user-option']} data-checked={checked || undefined}>
      <input
        type={question.type === 'multi' ? 'checkbox' : 'radio'}
        name={`ask-user-${question.key}`}
        className={styles['session-confirmation-field-checkbox']}
        checked={checked}
        disabled={busy}
        onChange={(event) => onToggle(event.target.checked)}
      />
      <span className={styles['ask-user-option-text']}>
        <span className={styles['ask-user-option-label']}>
          {option.label}
          {recommended && (
            <span className={styles['ask-user-option-badge']}>
              {textWithFallback('approval.askUser.recommended', '推荐')}
            </span>
          )}
        </span>
        {option.description && (
          <span className={styles['ask-user-option-desc']}>{option.description}</span>
        )}
      </span>
    </label>
  );
}

/** The batched questions an ask_user call is waiting on. */
export function AskUserForm({ form, values, busy, missingKeys, onChange }: AskUserFormProps) {
  const renderQuestion = (question: AskUserQuestion) => {
    const missing = missingKeys.has(question.key);
    const label = question.required ? `${question.question} *` : question.question;
    const current = values[question.key];
    return (
      <div key={question.key} className={styles['ask-user-question']} data-missing={missing || undefined}>
        <span className={styles['ask-user-question-label']}>
          {label}
          {question.type === 'multi' && (
            <span className={styles['ask-user-question-hint']}>
              {textWithFallback('approval.askUser.multiHint', '可多选')}
            </span>
          )}
        </span>
        {question.type === 'text' ? (
          <input
            type="text"
            aria-label={question.question}
            aria-invalid={missing}
            className={styles['session-confirmation-field-input']}
            value={typeof current === 'string' ? current : ''}
            disabled={busy}
            onChange={(event) => onChange(question.key, event.target.value)}
          />
        ) : (
          <div className={styles['ask-user-options']} role={question.type === 'multi' ? 'group' : 'radiogroup'} aria-label={question.question}>
            {question.options.map((option) => (
              <OptionRow
                key={option.value}
                question={question}
                option={option}
                busy={busy}
                checked={question.type === 'multi'
                  ? Array.isArray(current) && current.includes(option.value)
                  : current === option.value}
                onToggle={(checked) => {
                  if (question.type === 'multi') {
                    const base = Array.isArray(current) ? current : [];
                    onChange(
                      question.key,
                      checked ? [...base, option.value] : base.filter((v) => v !== option.value),
                    );
                  } else if (checked) {
                    onChange(question.key, option.value);
                  }
                }}
              />
            ))}
          </div>
        )}
      </div>
    );
  };

  return (
    <div className={styles['session-confirmation-form']}>
      {form.questions.map(renderQuestion)}
      {missingKeys.size > 0 && (
        <div className={styles['session-confirmation-field-required']} role="alert" data-testid="ask-user-required">
          {textWithFallback('approval.askUser.requiredMissing', '请回答标有 * 的问题')}
        </div>
      )}
      {form.unsupported.length > 0 && (
        <div
          className={styles['session-confirmation-field-unsupported']}
          data-testid="ask-user-unsupported"
        >
          {textWithFallback('approval.askUser.unsupportedQuestion', '暂不支持的问题形式')}
          {`: ${form.unsupported.join(', ')}`}
        </div>
      )}
    </div>
  );
}
