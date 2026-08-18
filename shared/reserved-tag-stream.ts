/**
 * ReservedTagScanner — 内部保留协议标签的流式扫描器
 *
 * 契约（第二阶段收口）：
 *   - 保留标签（如 <mood>/<pulse>/<reflect>、<think>/<thinking>）是协议，不是文本。
 *     无论出现在一次生成的什么位置，都必须被结构化，而不是只在开头才被识别。
 *   - 转义：`\<tag>` 与 `\</tag>` 输出字面量（反斜杠被消费，标签按普通文本透传）。
 *   - 代码保护：行内代码（1~2 个反引号成对）与围栏代码块（>=3 个反引号/波浪线）
 *     内部的标签一律按字面文本处理。
 *   - 跨 delta：半截标签、半截转义、半截代码标记都挂在缓冲里等下一段，绝不误吞。
 *   - 关闭标签必须与打开标签同名；标签内容不透明（内部不再识别代码/转义）。
 *
 * 输出 token 流：
 *   { type: "open", tag }   — 遇到完整、未转义、不在代码内的开标签
 *   { type: "close", tag }  — 与当前打开标签同名的闭标签
 *   { type: "text", text }  — 其余一切（含被转义/代码保护的字面标签）
 */

export type ReservedTagToken =
  | { type: "text"; text: string }
  | { type: "open"; tag: string }
  | { type: "close"; tag: string };

interface TagMatch {
  literal: string;
  tag: string;
  isOpen: boolean;
}

interface CodeSpan {
  marker: "`" | "~";
  run: number;
  kind: "inline" | "fence";
}

/** 检查 buffer 末尾是否是 target 的前缀（1..target.length-1 个字符），返回匹配长度 */
function trailingPrefixLen(buffer: string, target: string): number {
  const maxCheck = Math.min(buffer.length, target.length - 1);
  for (let len = maxCheck; len >= 1; len--) {
    if (buffer.endsWith(target.slice(0, len))) return len;
  }
  return 0;
}

export class ReservedTagScanner {
  private buffer = "";
  private openTag: string | null = null;
  private code: CodeSpan | null = null;
  private readonly literals: readonly string[];
  private readonly openers: ReadonlyMap<string, string>;

  constructor(tags: readonly string[]) {
    this.literals = Object.freeze(tags.flatMap((tag) => [`<${tag}>`, `</${tag}>`]));
    this.openers = new Map(tags.map((tag) => [`<${tag}>`, tag]));
  }

  /** 当前是否有打开的标签（供上层在 flush 时补发结束事件） */
  get insideTag(): string | null {
    return this.openTag;
  }

  feed(delta: string): ReservedTagToken[] {
    this.buffer += delta;
    return this.drain(false);
  }

  /** 冲刷缓冲：半截标签/转义/代码标记在此定界，按字面文本输出。 */
  flush(): ReservedTagToken[] {
    return this.drain(true);
  }

  reset(): void {
    this.buffer = "";
    this.openTag = null;
    this.code = null;
  }

  /**
   * 在 pos 处匹配完整标签字面量；返回 "partial" 表示 buffer 尾巴可能是
   * 跨 delta 的半截标签（仅在 pos 位于 buffer 末尾附近时才有意义）。
   */
  private matchTagAt(buf: string, pos: number): TagMatch | "partial" | null {
    for (const literal of this.literals) {
      if (buf.startsWith(literal, pos)) {
        const isOpen = !literal.startsWith("</");
        return { literal, tag: this.openers.get(literal) || literal.slice(2, -1), isOpen };
      }
    }
    const rest = buf.slice(pos);
    if (rest.startsWith("<")) {
      for (const literal of this.literals) {
        if (literal.length > rest.length && literal.startsWith(rest)) return "partial";
      }
    }
    return null;
  }

  private drain(isFlush: boolean): ReservedTagToken[] {
    const tokens: ReservedTagToken[] = [];
    let text = "";
    let i = 0;
    const buf = this.buffer;
    const pushText = () => {
      if (text) {
        tokens.push({ type: "text", text });
        text = "";
      }
    };

    while (i < buf.length) {
      // ── 标签内容模式：不透明，只找同名闭标签 ──
      if (this.openTag !== null) {
        const closeTag = `</${this.openTag}>`;
        const idx = buf.indexOf(closeTag, i);
        if (idx !== -1) {
          text += buf.slice(i, idx);
          pushText();
          tokens.push({ type: "close", tag: this.openTag });
          this.openTag = null;
          i = idx + closeTag.length;
          continue;
        }
        if (!isFlush) {
          const holdLen = trailingPrefixLen(buf.slice(i), closeTag);
          const safeEnd = buf.length - holdLen;
          text += buf.slice(i, safeEnd);
          pushText();
          this.buffer = buf.slice(safeEnd);
          return tokens;
        }
        text += buf.slice(i);
        pushText();
        this.buffer = "";
        return tokens;
      }

      const ch = buf[i];

      // ── 代码保护模式：只认对应的关闭标记 ──
      if (this.code) {
        if (ch === this.code.marker) {
          let run = 1;
          while (buf[i + run] === ch) run += 1;
          if (i + run >= buf.length && !isFlush) break; // 尾巴的标记可能变长，挂起
          text += buf.slice(i, i + run);
          i += run;
          const closes = this.code.kind === "fence"
            ? run >= this.code.run
            : run === this.code.run;
          if (closes) this.code = null;
          continue;
        }
        text += ch;
        i += 1;
        continue;
      }

      // ── 转义：\<tag> / \</tag> → 字面量，反斜杠被消费 ──
      if (ch === "\\") {
        if (i + 1 >= buf.length && !isFlush) break; // 可能在转义下一段的标签，挂起
        const match = this.matchTagAt(buf, i + 1);
        if (match === "partial" && !isFlush) break;
        if (match && match !== "partial") {
          text += match.literal;
          i += 1 + match.literal.length;
          continue;
        }
        text += ch;
        i += 1;
        continue;
      }

      // ── 代码标记：>=3 个反引号/波浪线是围栏；1~2 个反引号是行内代码 ──
      if (ch === "`" || ch === "~") {
        let run = 1;
        while (buf[i + run] === ch) run += 1;
        if (i + run >= buf.length && !isFlush) break; // 尾巴的标记可能变长，挂起
        text += buf.slice(i, i + run);
        i += run;
        if (run >= 3) this.code = { marker: ch as "`" | "~", run, kind: "fence" };
        else if (ch === "`") this.code = { marker: "`", run, kind: "inline" };
        continue;
      }

      // ── 标签 ──
      if (ch === "<") {
        const match = this.matchTagAt(buf, i);
        if (match === "partial" && !isFlush) break;
        if (match && match !== "partial") {
          if (match.isOpen) {
            pushText();
            tokens.push({ type: "open", tag: match.tag });
            this.openTag = match.tag;
          } else {
            // 没有打开标签时的孤立闭标签：按字面文本处理
            text += match.literal;
          }
          i += match.literal.length;
          continue;
        }
        text += ch;
        i += 1;
        continue;
      }

      text += ch;
      i += 1;
    }

    pushText();
    this.buffer = buf.slice(i);
    return tokens;
  }
}

export interface ReservedTagTextSegment {
  type: "text";
  text: string;
}

export interface ReservedTagBlockSegment {
  type: "block";
  tag: string;
  content: string;
}

export type ReservedTagSegment = ReservedTagTextSegment | ReservedTagBlockSegment;

/**
 * 一次性（非流式）把一段完整文本切成 文本/标签块 交替的片段序列。
 * 用于历史消息重渲染等"全文已在手"的场景；转义与代码保护规则与流式扫描一致。
 */
export function splitReservedTagSegments(
  content: string,
  tags: readonly string[],
): ReservedTagSegment[] {
  const scanner = new ReservedTagScanner(tags);
  const tokens = [...scanner.feed(content), ...scanner.flush()];
  const segments: ReservedTagSegment[] = [];
  let text = "";
  let block: { tag: string; content: string } | null = null;
  const pushText = () => {
    if (text) {
      segments.push({ type: "text", text });
      text = "";
    }
  };
  for (const token of tokens) {
    if (token.type === "open") {
      pushText();
      block = { tag: token.tag, content: "" };
    } else if (token.type === "close") {
      if (block) {
        segments.push({ type: "block", tag: block.tag, content: block.content });
        block = null;
      }
    } else if (block) {
      block.content += token.text;
    } else {
      text += token.text;
    }
  }
  // 未闭合的尾巴：扫描器 flush 已把内容作为 text 吐出，这里按文本保留
  if (block) text += `<${block.tag}>${block.content}`;
  pushText();
  return segments;
}
