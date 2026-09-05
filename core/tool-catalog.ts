/**
 * The tool catalog: what a session knows *about* tools it has not loaded.
 *
 * When a session defers tools, their schemas never enter the model's cacheable
 * prefix. The catalog is the replacement channel: it holds one cheap row per
 * tool and hands out detail only when the model asks through the bridge tools.
 *
 * Two properties are load-bearing:
 *
 * - The catalog never holds an executor or raw tool object. Entries may carry
 *   a read-only `schemaRef`, so deferred schemas need not enter the model's
 *   cacheable prefix.
 * - Scoring never reads the owning server. Provenance decides nothing about
 *   rank, so two equally matching tools from different servers score
 *   identically and no connector can buy attention with its label.
 *
 * Pure module: no I/O, no engine or manager dependency. Sources register and
 * replace their own slice; the catalog only merges and ranks.
 */

import { estimateTextTokens } from "../lib/llm/estimate-text-tokens.ts";
import {
  ToolInvocationError,
  type ToolOrigin,
  type ToolTargetId,
} from "../lib/tools/invocation/index.ts";

export type ToolCatalogOrigin = ToolOrigin;

interface ToolCatalogEntryInputBase {
  targetId: ToolTargetId;
  origin: ToolCatalogOrigin;
  sourceId: string;
  serverId: string;
  serverLabel: string;
  publicName: string;
  /** The local name known by the owning source. */
  toolName: string;
  capabilityBase: string;
  description: string;
  paramsSummary: string;
  lifecycleGeneration: string | number;
  deferrable: boolean;
  pinned: boolean;
}

export type ToolCatalogEntryInput = ToolCatalogEntryInputBase & (
  | { readonly schema: unknown; readonly schemaRef?: never }
  | { readonly schema?: never; readonly schemaRef: () => unknown }
);

export interface ToolCatalogEntry {
  readonly targetId: ToolTargetId;
  readonly origin: ToolCatalogOrigin;
  readonly sourceId: string;
  readonly serverId: string;
  readonly serverLabel: string;
  readonly publicName: string;
  /** Compatibility display alias. It is never used as execution identity. */
  readonly name: string;
  readonly toolName: string;
  readonly capabilityBase: string;
  readonly description: string;
  readonly paramsSummary: string;
  readonly lifecycleGeneration: string | number;
  readonly deferrable: boolean;
  readonly pinned: boolean;
  readonly schema?: unknown;
  readonly schemaRef?: () => unknown;
}

export interface ToolCatalogHit extends ToolCatalogEntry {
  readonly score: number;
}

export interface ToolCatalogManifest {
  readonly tier: 1 | 2;
  readonly text: string;
}

export interface ToolCatalogDescription {
  readonly targetId: ToolTargetId;
  readonly sourceId: string;
  readonly publicName: string;
  readonly toolName: string;
  readonly capabilityBase: string;
  readonly lifecycleGeneration: string | number;
  readonly schema: unknown;
  readonly paramsSummary: string;
  readonly serverId: string;
  readonly serverLabel: string;
  readonly name: string;
  readonly description: string;
  readonly origin: ToolCatalogOrigin;
}

export interface ToolCatalogTargetReference {
  readonly sourceId?: string;
  readonly serverId?: string;
  readonly toolName: string;
}

export interface ToolCatalogLookupQualifier {
  readonly sourceId?: string;
  readonly serverId?: string;
}

const BM25_K1 = 1.5;
const BM25_B = 0.75;
const DEFAULT_SEARCH_LIMIT = 5;
const DEFAULT_SEARCH_MAX = 20;
const MANIFEST_DESCRIPTION_CHARS = 120;

/**
 * Latin runs tokenize on any non-alphanumeric boundary, which splits
 * `github_create_issue` and `parent-id` the same way. CJK has no such
 * boundary, so each character becomes its own token; that is coarse but it
 * keeps Chinese descriptions searchable without pulling in a segmenter.
 */
function foldPlural(token: string): string {
  // Applied identically at index and query time, so folding never breaks a
  // match; at worst it merges two words that a lite ranker was never going to
  // tell apart. Short tokens and "ss" endings are left alone so "address" and
  // "bus" survive intact.
  if (token.length > 3 && token.endsWith("s") && !token.endsWith("ss")) {
    return token.slice(0, -1);
  }
  return token;
}

// Han, Hiragana and Katakana characters are indexed one character at a time.
// Written as script properties rather than literal ranges so the source stays
// free of the full-width punctuation those ranges would otherwise embed.
const CJK_CHAR_RE = /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}]/u;

function tokenize(value: unknown): string[] {
  if (typeof value !== "string" || !value) return [];
  const tokens: string[] = [];
  for (const run of value.toLowerCase().split(/[^\p{L}\p{N}]+/u)) {
    if (!run) continue;
    let latin = "";
    for (const char of run) {
      if (CJK_CHAR_RE.test(char)) {
        if (latin) {
          tokens.push(foldPlural(latin));
          latin = "";
        }
        tokens.push(char);
      } else {
        latin += char;
      }
    }
    if (latin) tokens.push(foldPlural(latin));
  }
  return tokens;
}

function normalizeText(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

function firstSentence(value: string, limit = MANIFEST_DESCRIPTION_CHARS): string {
  const flat = value.replace(/\s+/g, " ").trim();
  if (!flat) return "";
  const cut = flat.search(/[.。！!？?]\s|[.。！!？?]$/u);
  const sentence = cut >= 0 ? flat.slice(0, cut + 1) : flat;
  return sentence.length > limit ? `${sentence.slice(0, limit - 1)}…` : sentence;
}

function normalizeEntry(input: ToolCatalogEntryInput): ToolCatalogEntry {
  const targetId = normalizeText(input?.targetId) as ToolTargetId;
  const publicName = normalizeText(input?.publicName);
  const toolName = normalizeText(input?.toolName);
  const sourceId = normalizeText(input?.sourceId);
  const serverId = normalizeText(input.serverId);
  const capabilityBase = normalizeText(input?.capabilityBase);
  if (!targetId) throw new TypeError("tool catalog entry requires a targetId");
  if (!publicName) throw new TypeError(`tool catalog entry ${targetId} requires a publicName`);
  if (!toolName) throw new TypeError(`tool catalog entry ${targetId} requires a toolName`);
  if (!sourceId) throw new TypeError(`tool catalog entry ${targetId} requires a sourceId`);
  if (!serverId) throw new TypeError(`tool catalog entry ${targetId} requires a serverId`);
  if (!capabilityBase) throw new TypeError(`tool catalog entry ${targetId} requires a capabilityBase`);
  if (!(["first-party", "plugin", "mcp"] as const).includes(input?.origin)) {
    throw new TypeError(`tool catalog entry ${targetId} requires a valid origin`);
  }
  if (typeof input?.deferrable !== "boolean" || typeof input?.pinned !== "boolean") {
    throw new TypeError(`tool catalog entry ${targetId} requires explicit deferrable and pinned flags`);
  }
  if (typeof input?.lifecycleGeneration !== "string" && typeof input?.lifecycleGeneration !== "number") {
    throw new TypeError(`tool catalog entry ${targetId} requires a lifecycleGeneration`);
  }
  if (input.schema === undefined && typeof input.schemaRef !== "function") {
    throw new TypeError(`tool catalog entry ${targetId} requires schema or schemaRef`);
  }
  return Object.freeze({
    targetId,
    origin: input.origin,
    sourceId,
    serverId,
    serverLabel: normalizeText(input.serverLabel) || serverId,
    publicName,
    name: publicName,
    toolName,
    capabilityBase,
    description: normalizeText(input.description),
    paramsSummary: normalizeText(input.paramsSummary),
    lifecycleGeneration: input.lifecycleGeneration,
    deferrable: input.deferrable,
    pinned: input.pinned,
    ...(input.schema !== undefined ? { schema: input.schema } : {}),
    ...(typeof input.schemaRef === "function" ? { schemaRef: input.schemaRef } : {}),
  });
}

function catalogError(
  code: "TARGET_NOT_FOUND" | "TARGET_AMBIGUOUS",
  message: string,
  details: Record<string, unknown>,
): ToolInvocationError {
  return new ToolInvocationError({ code, message, route: "deferred", details });
}

interface ScoringDoc {
  readonly entry: ToolCatalogEntry;
  readonly frequencies: ReadonlyMap<string, number>;
  readonly length: number;
}

export class ToolCatalog {
  private readonly _sources = new Map<string, ToolCatalogEntry[]>();
  private _index: Map<ToolTargetId, ToolCatalogEntry> | null = null;
  private _docs: ScoringDoc[] | null = null;

  registerSource(sourceId: string, entries: readonly ToolCatalogEntryInput[]): void {
    const id = normalizeText(sourceId);
    if (!id) throw new TypeError("tool catalog registerSource requires a sourceId");
    const normalized = (Array.isArray(entries) ? entries : []).map(normalizeEntry);
    const proposed = new Map(this._sources);
    proposed.set(id, normalized);
    this._validateEntries([...proposed.values()].flat());
    this._sources.set(id, normalized);
    this._invalidate();
  }

  /** Same as registerSource; named for the refresh call sites that swap a slice. */
  replaceSource(sourceId: string, entries: readonly ToolCatalogEntryInput[]): void {
    this.registerSource(sourceId, entries);
  }

  removeSource(sourceId: string): boolean {
    const removed = this._sources.delete(normalizeText(sourceId));
    if (removed) this._invalidate();
    return removed;
  }

  clear(): void {
    this._sources.clear();
    this._invalidate();
  }

  size(): number {
    return this._entryIndex().size;
  }

  has(name: string): boolean {
    return this._matchingEntries({ toolName: name }).length > 0;
  }

  get(name: string, qualifier: ToolCatalogLookupQualifier = {}): ToolCatalogEntry | null {
    return this._resolveEntry({ ...qualifier, toolName: name }, false);
  }

  getByTargetId(targetId: ToolTargetId): ToolCatalogEntry | null {
    return this._entryIndex().get(targetId) ?? null;
  }

  resolveTarget(reference: ToolCatalogTargetReference): ToolTargetId {
    return this._resolveEntry(reference, true)!.targetId;
  }

  all(): ToolCatalogEntry[] {
    return [...this._entryIndex().values()];
  }

  /** Tool names currently in the catalog, sorted, for change diffing. */
  names(): string[] {
    return this.all().map((entry) => entry.publicName).sort((left, right) => left.localeCompare(right));
  }

  search(
    query: string,
    { limit = DEFAULT_SEARCH_LIMIT, max = DEFAULT_SEARCH_MAX }: { limit?: number; max?: number } = {},
  ): ToolCatalogHit[] {
    const cap = Math.max(1, Math.min(
      Number.isFinite(limit) ? Math.floor(limit as number) : DEFAULT_SEARCH_LIMIT,
      Number.isFinite(max) ? Math.floor(max as number) : DEFAULT_SEARCH_MAX,
    ));
    const queryTokens = tokenize(query);
    const docs = this._scoringDocs();
    if (docs.length === 0) return [];

    const scored: ToolCatalogHit[] = [];
    if (queryTokens.length > 0) {
      const totalDocs = docs.length;
      const averageLength = docs.reduce((sum, doc) => sum + doc.length, 0) / totalDocs || 1;
      const documentFrequency = new Map<string, number>();
      for (const token of new Set(queryTokens)) {
        let count = 0;
        for (const doc of docs) if (doc.frequencies.has(token)) count += 1;
        documentFrequency.set(token, count);
      }
      for (const doc of docs) {
        let score = 0;
        for (const token of queryTokens) {
          const frequency = doc.frequencies.get(token);
          if (!frequency) continue;
          const seen = documentFrequency.get(token) ?? 0;
          const idf = Math.log(1 + (totalDocs - seen + 0.5) / (seen + 0.5));
          const denominator = frequency + BM25_K1 * (1 - BM25_B + (BM25_B * doc.length) / averageLength);
          score += idf * ((frequency * (BM25_K1 + 1)) / denominator);
        }
        if (score > 0) scored.push(Object.freeze({ ...doc.entry, score }));
      }
    }

    if (scored.length === 0) return this._substringFallback(query, queryTokens, cap);

    scored.sort((left, right) => (right.score - left.score) || left.name.localeCompare(right.name));
    return scored.slice(0, cap);
  }

  manifest(budgetTokens: number): ToolCatalogManifest {
    const groups = this._groupedByServer();
    if (groups.length === 0) return { tier: 1, text: "" };

    const tierOne = groups.map(({ label, entries }) => {
      const header = `${label}（${entries.length}）`;
      const lines = entries.map((entry) => {
        const summary = firstSentence(entry.description) || "无描述";
        return `- ${entry.name} — ${summary}${entry.pinned ? "（已加载）" : ""}`;
      });
      return [header, ...lines].join("\n");
    }).join("\n\n");

    const budget = Number.isFinite(budgetTokens) ? Math.max(0, Math.floor(budgetTokens)) : 0;
    if (estimateTextTokens(tierOne) <= budget) return { tier: 1, text: tierOne };

    const tierTwo = groups
      .map(({ label, entries }) => `- ${label}（${entries.length} 个工具，可搜索）`)
      .join("\n");
    return { tier: 2, text: tierTwo };
  }

  describe(name: string, qualifier: ToolCatalogLookupQualifier = {}): ToolCatalogDescription | null {
    const entry = this.get(name, qualifier);
    if (!entry) return null;
    let schema: unknown = null;
    try {
      schema = entry.schema !== undefined ? entry.schema : entry.schemaRef?.() ?? null;
    } catch {
      // A source whose schema resolution has gone away still deserves a usable
      // row; the caller renders the summary and the model can still call it.
      schema = null;
    }
    return Object.freeze({
      targetId: entry.targetId,
      sourceId: entry.sourceId,
      publicName: entry.publicName,
      name: entry.name,
      toolName: entry.toolName,
      capabilityBase: entry.capabilityBase,
      lifecycleGeneration: entry.lifecycleGeneration,
      description: entry.description,
      schema,
      paramsSummary: entry.paramsSummary,
      serverId: entry.serverId,
      serverLabel: entry.serverLabel,
      origin: entry.origin,
    });
  }

  private _substringFallback(query: string, queryTokens: string[], cap: number): ToolCatalogHit[] {
    const needles = [normalizeText(query).toLowerCase(), ...queryTokens].filter(Boolean);
    if (needles.length === 0) return [];
    const hits: ToolCatalogHit[] = [];
    for (const entry of this._entryIndex().values()) {
      const haystack = entry.name.toLowerCase();
      if (needles.some((needle) => haystack.includes(needle))) {
        hits.push(Object.freeze({ ...entry, score: 0 }));
      }
    }
    hits.sort((left, right) => left.name.localeCompare(right.name));
    return hits.slice(0, cap);
  }

  private _groupedByServer(): { serverId: string; label: string; entries: ToolCatalogEntry[] }[] {
    const groups = new Map<string, { serverId: string; label: string; entries: ToolCatalogEntry[] }>();
    for (const entry of this._entryIndex().values()) {
      let group = groups.get(entry.serverId);
      if (!group) {
        group = { serverId: entry.serverId, label: entry.serverLabel, entries: [] };
        groups.set(entry.serverId, group);
      }
      group.entries.push(entry);
    }
    const ordered = [...groups.values()];
    for (const group of ordered) {
      group.entries.sort((left, right) => left.name.localeCompare(right.name));
    }
    ordered.sort((left, right) => left.label.localeCompare(right.label) || left.serverId.localeCompare(right.serverId));
    return ordered;
  }

  private _invalidate(): void {
    this._index = null;
    this._docs = null;
  }

  private _entryIndex(): Map<ToolTargetId, ToolCatalogEntry> {
    if (this._index) return this._index;
    const index = new Map<ToolTargetId, ToolCatalogEntry>();
    for (const entries of this._sources.values()) {
      for (const entry of entries) {
        index.set(entry.targetId, entry);
      }
    }
    this._index = index;
    return index;
  }

  private _matchingEntries(reference: ToolCatalogTargetReference): ToolCatalogEntry[] {
    const toolName = normalizeText(reference?.toolName);
    if (!toolName) return [];
    const sourceId = normalizeText(reference?.sourceId);
    const serverId = normalizeText(reference?.serverId);
    return this.all().filter((entry) => (
      (!sourceId || entry.sourceId === sourceId)
      && (!serverId || entry.serverId === serverId)
      && (entry.publicName === toolName || entry.toolName === toolName)
    ));
  }

  private _resolveEntry(
    reference: ToolCatalogTargetReference,
    failWhenMissing: boolean,
  ): ToolCatalogEntry | null {
    const matches = this._matchingEntries(reference);
    if (matches.length === 1) return matches[0];
    const details = {
      sourceId: normalizeText(reference?.sourceId) || null,
      serverId: normalizeText(reference?.serverId) || null,
      toolName: normalizeText(reference?.toolName),
      matches: matches.map((entry) => entry.targetId).sort(),
    };
    if (matches.length > 1) {
      throw catalogError("TARGET_AMBIGUOUS", "Tool catalog reference matches more than one target.", details);
    }
    if (failWhenMissing) {
      throw catalogError("TARGET_NOT_FOUND", "No tool catalog target matches this reference.", details);
    }
    return null;
  }

  private _validateEntries(entries: ToolCatalogEntry[]): void {
    const targetIds = new Set<ToolTargetId>();
    const sourceNames = new Map<string, ToolTargetId>();
    for (const entry of entries) {
      if (targetIds.has(entry.targetId)) {
        throw catalogError("TARGET_AMBIGUOUS", "Tool catalog targetId is registered more than once.", {
          targetId: entry.targetId,
        });
      }
      targetIds.add(entry.targetId);
      for (const name of new Set([entry.publicName, entry.toolName])) {
        const key = JSON.stringify([entry.sourceId, name]);
        const existing = sourceNames.get(key);
        if (existing && existing !== entry.targetId) {
          throw catalogError("TARGET_AMBIGUOUS", "Tool catalog source contains a duplicate tool name.", {
            sourceId: entry.sourceId,
            toolName: name,
            targetIds: [existing, entry.targetId].sort(),
          });
        }
        sourceNames.set(key, entry.targetId);
      }
    }
  }

  private _scoringDocs(): ScoringDoc[] {
    if (this._docs) return this._docs;
    const docs: ScoringDoc[] = [];
    for (const entry of this._entryIndex().values()) {
      // Name, description and parameter names only. The server id and label are
      // deliberately absent so provenance cannot influence rank.
      const tokens = [
        ...tokenize(entry.name),
        ...tokenize(entry.description),
        ...tokenize(entry.paramsSummary),
      ];
      const frequencies = new Map<string, number>();
      for (const token of tokens) frequencies.set(token, (frequencies.get(token) ?? 0) + 1);
      docs.push({ entry, frequencies, length: tokens.length });
    }
    this._docs = docs;
    return docs;
  }
}

export function createToolCatalog(): ToolCatalog {
  return new ToolCatalog();
}

export interface ToolCatalogDiff {
  readonly added: string[];
  readonly removed: string[];
}

/**
 * Compares two catalog name snapshots. Used to notice that a server's tool
 * listing changed after a refresh, so the session can be told rather than
 * silently holding a stale listing.
 */
export function diffCatalogNames(before: readonly string[], after: readonly string[]): ToolCatalogDiff {
  const beforeSet = new Set(Array.isArray(before) ? before : []);
  const afterSet = new Set(Array.isArray(after) ? after : []);
  return {
    added: [...afterSet].filter((name) => !beforeSet.has(name)).sort((l, r) => l.localeCompare(r)),
    removed: [...beforeSet].filter((name) => !afterSet.has(name)).sort((l, r) => l.localeCompare(r)),
  };
}

/**
 * Renders a catalog change as broadcast lines. These go through the ordinary
 * reminder channel, which is short by design: the session is being told that
 * something moved, not handed a new listing. It can search for the details.
 */
export function formatCatalogChangeLines(diff: ToolCatalogDiff, isZh: boolean): string[] {
  const lines: string[] = [];
  if (diff.added.length > 0) {
    lines.push(isZh
      ? `外部工具新增：${diff.added.join("、")}`
      : `External tools added: ${diff.added.join(", ")}`);
  }
  if (diff.removed.length > 0) {
    lines.push(isZh
      ? `外部工具移除：${diff.removed.join("、")}`
      : `External tools removed: ${diff.removed.join(", ")}`);
  }
  return lines;
}
