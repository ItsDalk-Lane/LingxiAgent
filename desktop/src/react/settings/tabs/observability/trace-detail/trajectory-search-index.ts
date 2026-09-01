/**
 * trajectory-search-index.ts — 轨迹台账的增量全文索引。
 *
 * 移植自 dsh-desktop packages/client/ui-trajectory/src/client/trajectory-search-index.ts
 * （MIT），原样移植（import 站点改本地）。
 */

import type { TrajectoryTurnModel } from './trace-layout.ts';
import type { TrajectoryCellProps } from './trajectory-record.ts';
import { trajectoryRecordId } from './trajectory-record.ts';
import { trajectoryPreviewText } from './trajectory-preview.ts';

interface SearchEntry {
  readonly sources: readonly string[];
  readonly text: string;
}

function searchableJson(value: unknown): string {
  if (value === undefined) return '';
  try {
    return JSON.stringify(value);
  } catch {
    return '';
  }
}

function sameSources(left: readonly string[], right: readonly string[]): boolean {
  return left.length === right.length && left.every((value, index) => right[index] === value);
}

function markdownPreview(cell: TrajectoryCellProps): string {
  if (cell.previewMarkdown === undefined) return '';
  const preview = trajectoryPreviewText(cell.previewMarkdown);
  if (cell.text === '') return preview;
  return preview === '' ? cell.text : `${cell.text} · ${preview}`;
}

function resultPreview(cell: TrajectoryCellProps): string {
  return cell.resultPreviewMarkdown === undefined
    ? cell.result ?? ''
    : trajectoryPreviewText(cell.resultPreviewMarkdown);
}

function recordSources(
  turn: number | null,
  group: string,
  cell: TrajectoryCellProps,
): readonly string[] {
  const blocks = [
    ...(cell.sourceBlocks ?? []),
    ...(cell.outputBlocks ?? []),
  ];
  return [
    turn === null ? 'between turns' : `turn ${turn}`,
    group,
    cell.kind,
    cell.kind === 'message' ? 'assistant' : '',
    cell.text,
    cell.previewMarkdown ?? '',
    cell.inputDetail ?? '',
    cell.outputDetail ?? '',
    cell.thinkingDetail ?? '',
    cell.schemaDetail ?? '',
    cell.result ?? '',
    cell.resultPreviewMarkdown ?? '',
    cell.callId ?? '',
    cell.observabilityCallId ?? '',
    ...blocks.flatMap(block => [
      block.type,
      block.content,
      block.callId ?? '',
      block.toolName ?? '',
      block.imageAlt ?? '',
    ]),
    searchableJson(cell.messageSource),
    searchableJson(cell.promptDetail),
    searchableJson(cell.previousPromptDetail),
  ];
}

/** 视图本地索引：仅在单条记录的源变化时才重解析 Markdown。 */
export class TrajectorySearchIndex {
  private readonly entries = new Map<string, SearchEntry>();
  private layouts: readonly (readonly TrajectoryTurnModel[])[] | undefined;

  /**
   * 增量同步一或多份当前轨迹布局切片。
   */
  update(layouts: readonly (readonly TrajectoryTurnModel[])[]): boolean {
    if (this.layouts === layouts) return false;
    this.layouts = layouts;
    const seen = new Set<string>();
    for (const turns of layouts) {
      for (const turn of turns) {
        for (const group of turn.groups) {
          for (const cell of group.cells) {
            if (cell.requestOnly === true) continue;
            const id = trajectoryRecordId(cell);
            const sources = recordSources(turn.turn, group.title, cell);
            const previous = this.entries.get(id);
            const entry = previous !== undefined && sameSources(previous.sources, sources)
              ? previous
              : {
                sources,
                text: [
                  ...sources,
                  markdownPreview(cell),
                  resultPreview(cell),
                ].join('\n').toLocaleLowerCase(),
              };
            this.entries.set(id, entry);
            seen.add(id);
          }
        }
      }
    }
    for (const id of this.entries.keys()) {
      if (!seen.has(id)) this.entries.delete(id);
    }
    return true;
  }

  /**
   * 用查询匹配最近提交的索引版本。
   */
  search(query: string): ReadonlySet<string> | null {
    const terms = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
    if (terms.length === 0) return null;
    const matches = new Set<string>();
    for (const [id, entry] of this.entries) {
      if (terms.every(term => entry.text.includes(term))) matches.add(id);
    }
    return matches;
  }
}
