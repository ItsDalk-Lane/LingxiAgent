/**
 * ConversationMapDraftCard — 画布上的草稿卡片（继续追问 / 创建分支）
 *
 * 虚线边框沿用泳道色；快捷短语存 localStorage，可在编辑模式下增删。
 */

import { useEffect, useMemo, useState } from 'react';
import type { CSSProperties } from 'react';
import { useStore } from '../../stores';
import type { MapDraft } from '../../stores/conversation-map-slice';
import { useI18n } from '../../hooks/use-i18n';
import { CARD_HEIGHT, CARD_WIDTH, type MapCard } from './conversation-map-layout';
import { sendMapDraft } from './conversation-map-send';
import styles from './ConversationMap.module.css';

const QUICK_PHRASES_KEY = 'lingxi-map:quick-phrases';
const MAX_PHRASES = 12;
const MAX_PHRASE_LENGTH = 16;

function normalizePhrases(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const seen = new Set<string>();
  const out: string[] = [];
  for (const item of value) {
    if (typeof item !== 'string') continue;
    const phrase = item.trim().slice(0, MAX_PHRASE_LENGTH);
    if (!phrase || seen.has(phrase)) continue;
    seen.add(phrase);
    out.push(phrase);
    if (out.length >= MAX_PHRASES) break;
  }
  return out;
}

function loadQuickPhrases(fallback: string[]): string[] {
  try {
    const raw = localStorage.getItem(QUICK_PHRASES_KEY);
    if (raw) {
      const parsed = normalizePhrases(JSON.parse(raw));
      if (parsed.length > 0) return parsed;
    }
  } catch {
    // 损坏的本地缓存回退到默认短语
  }
  return fallback;
}

function saveQuickPhrases(phrases: string[]): void {
  try {
    localStorage.setItem(QUICK_PHRASES_KEY, JSON.stringify(phrases.slice(0, MAX_PHRASES)));
  } catch {
    // 存储不可用时不阻塞编辑
  }
}

interface Props {
  draft: MapDraft;
  anchor: MapCard;
  position: { x: number; y: number };
}

export function ConversationMapDraftCard({ draft, anchor, position }: Props) {
  const { t } = useI18n();
  const setMapDraft = useStore((s) => s.setMapDraft);
  const updateMapDraftText = useStore((s) => s.updateMapDraftText);
  const setMapDraftSending = useStore((s) => s.setMapDraftSending);

  const defaultPhrases = useMemo(
    () => normalizePhrases(t('map.quickPhrases').split('|')),
    [t],
  );
  const [phrases, setPhrases] = useState<string[]>(() => loadQuickPhrases(defaultPhrases));
  const [editing, setEditing] = useState(false);
  const [newPhrase, setNewPhrase] = useState('');

  // locale 切换后若用户从未自定义，跟随默认短语。
  useEffect(() => {
    try {
      if (!localStorage.getItem(QUICK_PHRASES_KEY)) setPhrases(defaultPhrases);
    } catch {
      /* ignore */
    }
  }, [defaultPhrases]);

  const updatePhrases = (next: string[]) => {
    setPhrases(next);
    saveQuickPhrases(next);
  };

  const appendPhrase = (phrase: string) => {
    if (draft.sending) return;
    const next = `${draft.text}${draft.text.endsWith(' ') || !draft.text ? '' : ' '}${phrase}`;
    updateMapDraftText(next.slice(0, 4000));
  };

  const addPhrase = () => {
    const phrase = newPhrase.trim().slice(0, MAX_PHRASE_LENGTH);
    if (!phrase || phrases.includes(phrase) || phrases.length >= MAX_PHRASES) return;
    updatePhrases([...phrases, phrase]);
    setNewPhrase('');
  };

  const send = async () => {
    if (draft.sending || !draft.text.trim()) return;
    setMapDraftSending(true);
    try {
      const ok = await sendMapDraft(draft, {
        answerEntryId: anchor.answerEntryId,
        questionEntryId: anchor.questionEntryId,
      }, t);
      if (ok) setMapDraft(null);
      else setMapDraftSending(false);
    } catch (err) {
      console.warn('[conversation-map] send draft failed', err);
      useStore.getState().addToast(t('map.sendFailed'), 'error');
      setMapDraftSending(false);
    }
  };

  const style: CSSProperties = {
    left: position.x,
    top: position.y,
    width: CARD_WIDTH,
    minHeight: CARD_HEIGHT,
    ['--card-accent' as string]: anchor.color,
  };

  return (
    <div className={styles.draftCard} style={style} data-map-card="" data-map-draft="">
      <div className={styles.cardHeader}>
        <span className={styles.cardDot} style={{ background: anchor.color }} />
        <span className={styles.cardTitle}>
          {draft.kind === 'continue' ? t('map.continue') : t('map.branch')}
        </span>
      </div>
      <div className={styles.draftBody}>
        <textarea
          className={styles.draftTextarea}
          value={draft.text}
          maxLength={4000}
          autoFocus
          disabled={draft.sending}
          placeholder={draft.kind === 'continue'
            ? t('map.draftContinuePlaceholder')
            : t('map.draftBranchPlaceholder')}
          onChange={(event) => updateMapDraftText(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
              event.preventDefault();
              void send();
            }
          }}
        />
        <div className={styles.chipRow}>
          {phrases.map((phrase) => (
            <button
              key={phrase}
              type="button"
              className={styles.chip}
              disabled={draft.sending}
              onClick={() => {
                if (editing) updatePhrases(phrases.filter((p) => p !== phrase));
                else appendPhrase(phrase);
              }}
            >
              {phrase}{editing && <span className={styles.chipRemove}>✕</span>}
            </button>
          ))}
          <button
            type="button"
            className={styles.chip}
            title={t('map.editPhrases')}
            disabled={draft.sending}
            onClick={() => setEditing((prev) => !prev)}
          >
            ✎
          </button>
        </div>
        {editing && (
          <div className={styles.chipEditRow}>
            <input
              className={styles.chipInput}
              value={newPhrase}
              maxLength={MAX_PHRASE_LENGTH}
              placeholder={t('map.editPhrases')}
              onChange={(event) => setNewPhrase(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault();
                  addPhrase();
                }
              }}
            />
            <button type="button" className={styles.chip} onClick={addPhrase}>
              {t('map.addPhrase')}
            </button>
          </div>
        )}
        <div className={styles.draftActions}>
          <button
            type="button"
            className={styles.toolbarButton}
            disabled={draft.sending}
            onClick={() => setMapDraft(null)}
          >
            {t('map.cancel')}
          </button>
          <button
            type="button"
            className={`${styles.toolbarButton} ${styles.draftSendButton}`}
            disabled={draft.sending || !draft.text.trim()}
            onClick={() => void send()}
          >
            {draft.sending ? t('map.sending') : t('map.send')}
          </button>
        </div>
      </div>
    </div>
  );
}
