import { useStore } from '../../stores';
import type { QuotedSelection } from '../../stores/input-slice';
import { AttachmentChip } from '../shared/AttachmentChip';

export function QuotedSelectionCard({selections, onRemove}: {selections?: readonly QuotedSelection[]; onRemove?: (index: number) => void} = {}) {
  const primarySelections = useStore(s => s.quotedSelections);
  const primaryRemove = useStore(s => s.removeQuotedSelection);
  const quotedSelections = selections ?? primarySelections;
  const removeQuotedSelection = onRemove ?? primaryRemove;

  if (quotedSelections.length === 0) return null;

  return (
    <>
      {quotedSelections.map((selection, index) => (
        <AttachmentChip
          key={`${selection.sourceKind}:${selection.sourceFilePath || selection.sourceSessionPath || ''}:${selection.sourceMessageId || ''}:${selection.updatedAt || index}`}
          icon={<TextCursorIcon />}
          name={selection.text}
          onRemove={() => removeQuotedSelection(index)}
        />
      ))}
    </>
  );
}

function TextCursorIcon() {
  return (
    <svg
      data-icon="text-cursor"
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.7"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M8 5h8" />
      <path d="M12 5v14" />
      <path d="M8 19h8" />
    </svg>
  );
}
