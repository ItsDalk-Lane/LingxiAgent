/**
 * ReservedTagScanner — 内部保留协议标签的流式扫描器
 *
 * 契约（F8/P6 三概念分离）：
 *   1. 已知内部协议成对标签（如 <mood>/<pulse>/<reflect>、<think>/<thinking>）
 *      是协议，不是文本：无论出现在一次生成的什么位置都结构化。
 *   2. 正常未知标记（<details>、<x:item a="1>0">、<custom/>、注释、CDATA 等）
 *      按通用形状规则维护「开启栈」：配对的原样保留；只有真正没有开启记录的
 *      未知闭标签才是协议残渣，仅在最终原始 assistant 文本边界清理。
 *   3. 转义与代码保护：`\<tag>`（任意标签形状）与行内代码/围栏代码块内的
 *      标签一律按字面文本处理，反斜杠随字面量保留到显示层
 *      （stripTagEscapes）一次性消费——任何中间层都不得吞掉受保护字面量。
 *
 * 三个扫描状态分开保存：协议块 openTag、未知标记栈 unknownStack、代码 code。
 *
 * 跨 delta：半截标签（含引号属性内的 >）、半截转义、半截代码标记都挂起等待，
 * flush 时按字面定界。有界保护：未知栈深度与挂起缓冲长度达到上限后转入
 * 保守透传（不再做孤儿清理，绝不成段丢正文）。
 *
 * 输出 token 流：
 *   { type: "open", tag }   — 遇到完整、未转义、不在代码内的已知协议开标签
 *   { type: "close", tag }  — 与当前打开协议标签同名的闭标签
 *   { type: "text", text }  — 其余一切（未知标记、被转义/代码保护的字面标签）
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

/** 通用标记形状匹配结果（F8/P6.2）。length 始终是完整字面量长度。 */
type GenericTagMatch =
  | { kind: "open"; name: string; length: number }
  | { kind: "close"; name: string; length: number }
  | { kind: "self-close"; name: string; length: number }
  | { kind: "comment"; length: number }
  | { kind: "cdata"; length: number }
  | { kind: "decl"; length: number }
  | "partial";

/** 检查 buffer 末尾是否是 target 的前缀（1..target.length-1 个字符），返回匹配长度 */
function trailingPrefixLen(buffer: string, target: string): number {
  const maxCheck = Math.min(buffer.length, target.length - 1);
  for (let len = maxCheck; len >= 1; len--) {
    if (buffer.endsWith(target.slice(0, len))) return len;
  }
  return 0;
}

const TAG_NAME_PATTERN = /[A-Za-z][A-Za-z0-9:_.-]*/y;
/** 词表外孤儿闭标签的完整形状：`</name>`（保持原有支持的名字字符集，含命名空间方言）。 */
const CLOSE_TAG_SHAPE = /^<\/([A-Za-z][A-Za-z0-9:_.-]*)>/;
/** 跨 delta 挂起判定：尾巴是尚未闭合的 `</…` 或 `<…` 标签形状（还没有出现 `>`）。 */
const PARTIAL_TAG_SHAPE = /^<\/?[A-Za-z][A-Za-z0-9:_.-]*$/;

/** 未结束标记的有界解析保护（F8/P6.4）：挂起缓冲超过上限后按字面透传。 */
const MAX_PENDING_TAG_LEN = 16 * 1024;
/** 未知标记栈深度上限：达到后本段转入保守透传并停止激进清理。 */
const MAX_TAG_STACK_DEPTH = 128;

function isSpace(ch: string): boolean {
  return ch === " " || ch === "\t" || ch === "\n" || ch === "\r" || ch === "\f";
}

/**
 * 在 buf[pos]（必须是 '<'）处按通用文本标记语法匹配一个完整标记。
 * - 引号属性值内的 `>` 不是终止符；
 * - `<!-- … -->` 注释、`<![CDATA[ … ]]>`、`<? … ?>`/`<! … >` 声明不建立开启状态；
 * - `<name … />` 自闭合不入栈；
 * - 无法确定（EOF 附近）返回 "partial"；形状不成立（如 `<3`、`< 看`）返回 null。
 * 这里只做标准文本标记语法判断，不建立任何供应商协议名白名单；名字大小写敏感。
 */
function matchGenericTag(buf: string, pos: number): GenericTagMatch | "partial" | null {
  if (buf[pos] !== "<") return null;
  const rest = buf.slice(pos);

  if (rest.startsWith("<!--")) {
    const end = rest.indexOf("-->", 4);
    return end === -1 ? "partial" : { kind: "comment", length: end + 3 };
  }
  if (rest.startsWith("<![CDATA[")) {
    const end = rest.indexOf("]]>", 9);
    return end === -1 ? "partial" : { kind: "cdata", length: end + 3 };
  }
  if (rest.startsWith("<?") || rest.startsWith("<!")) {
    const end = rest.indexOf(">");
    return end === -1 ? "partial" : { kind: "decl", length: end + 1 };
  }

  if (rest.startsWith("</")) {
    TAG_NAME_PATTERN.lastIndex = 2;
    const name = TAG_NAME_PATTERN.exec(rest)?.[0];
    if (!name) return null;
    const after = rest[2 + name.length];
    if (after === undefined) return "partial";
    if (after !== ">") return null;
    return { kind: "close", name, length: 2 + name.length + 1 };
  }

  TAG_NAME_PATTERN.lastIndex = 1;
  const name = TAG_NAME_PATTERN.exec(rest)?.[0];
  if (!name) return null;

  let i = 1 + name.length;
  let selfClose = false;
  for (;;) {
    if (i >= rest.length) return "partial";
    const ch = rest[i];
    if (ch === ">") break;
    if (ch === "/") {
      if (rest[i + 1] === ">") {
        selfClose = true;
        i += 2;
        break;
      }
      return null;
    }
    if (isSpace(ch)) {
      i += 1;
      continue;
    }
    // 属性名：吃到空白/=/>// 之外
    let moved = false;
    while (i < rest.length && !isSpace(rest[i]) && rest[i] !== "=" && rest[i] !== ">" && rest[i] !== "/") {
      i += 1;
      moved = true;
    }
    if (!moved) return null;
    if (i >= rest.length) return "partial";
    if (rest[i] !== "=") continue;
    i += 1;
    if (i >= rest.length) return "partial";
    const quote = rest[i];
    if (quote === '"' || quote === "'") {
      // 引号内的 > 不是终止符
      const closeQuote = rest.indexOf(quote, i + 1);
      if (closeQuote === -1) return "partial";
      i = closeQuote + 1;
      continue;
    }
    // 未加引号的值：吃到空白或 >
    while (i < rest.length && !isSpace(rest[i]) && rest[i] !== ">") i += 1;
  }
  return { kind: selfClose ? "self-close" : "open", name, length: i };
}

/** 完整标签字面量（含反斜杠）→ 受保护文本；显示层用 stripTagEscapes 消费反斜杠。 */

/** 显示层一次性消费转义反斜杠：`\` + 完整标记形状 → 标记字面量。与扫描器的
 * 转义判定同源（matchGenericTag），引号属性内的 > 不会被误当边界。幂等。 */
export function stripTagEscapes(text: string): string {
  let out = "";
  let i = 0;
  while (i < text.length) {
    if (text[i] === "\\" && text[i + 1] === "<") {
      const generic = matchGenericTag(text, i + 1);
      if (generic && generic !== "partial") {
        out += text.slice(i + 1, i + 1 + generic.length);
        i += 1 + generic.length;
        continue;
      }
    }
    out += text[i];
    i += 1;
  }
  return out;
}

export interface ReservedTagScannerOptions {
  /**
   * 词表外孤儿闭标签形状规则（方言无关兜底）：没有开启记录（协议栈与未知
   * 标记栈都没有对应名字）时遇到的未知收尾标签，按模型协议残渣吞掉。
   * 词表内的孤儿闭标签保持字面透传契约（转义/教学场景依赖）。
   *
   * 只在「最终文本边界」开启：解析链的中间层（如 ThinkTagParser 链在
   * MoodParser 之前）不认识下游的结构标签（`</mood>` 对 think 层就是
   * 「未知名字的闭标签」，但 think 层的未知标记栈记录了 `<mood>` 的开启），
   * 中间层保持透传，孤儿清理由链尾（MoodParser）或独立全文切分兜底。
   */
  dropUnknownOrphanClosers?: boolean;
}

export class ReservedTagScanner {
  private buffer = "";
  private openTag: string | null = null;
  private code: CodeSpan | null = null;
  /** 正常未知标记的开启栈（F8/P6.2 第二概念）。 */
  private unknownStack: string[] = [];
  /** 有界保护触发后：本段保守透传，不再做孤儿清理。 */
  private conservativePassthrough = false;
  private readonly literals: readonly string[];
  private readonly openers: ReadonlyMap<string, string>;
  private readonly dropUnknownOrphanClosers: boolean;

  constructor(tags: readonly string[], options: ReservedTagScannerOptions = {}) {
    this.literals = Object.freeze(tags.flatMap((tag) => [`<${tag}>`, `</${tag}>`]));
    this.openers = new Map(tags.map((tag) => [`<${tag}>`, tag]));
    this.dropUnknownOrphanClosers = options.dropUnknownOrphanClosers === true;
  }

  /** 当前是否有打开的协议标签（供上层在 flush 时补发结束事件） */
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
    this.unknownStack = [];
    this.conservativePassthrough = false;
  }

  /**
   * 在 pos 处匹配完整协议标签字面量；返回 "partial" 表示 buffer 尾巴可能是
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
      // ── 协议块内容模式：不透明，只找同名闭标签 ──
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

      // ── 转义：`\<任意标签形状>` → 受保护字面量（反斜杠保留到显示层消费）──
      if (ch === "\\") {
        if (i + 1 >= buf.length && !isFlush) break; // 可能在转义下一段的标签，挂起
        if (buf[i + 1] === "<") {
          const generic = matchGenericTag(buf, i + 1);
          if (generic === "partial" && !isFlush) break;
          if (generic && generic !== "partial") {
            text += buf.slice(i, i + 1 + generic.length);
            i += 1 + generic.length;
            continue;
          }
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
            // 词表内的孤儿闭标签：保持旧的字面透传契约（转义/教学场景依赖）；
            // 残渣兜底交给链尾扫描器的「词表外孤儿闭标签」规则。
            text += match.literal;
          }
          i += match.literal.length;
          continue;
        }
        const generic = matchGenericTag(buf, i);
        if (generic === "partial" && !isFlush) {
          // 有界保护：挂起区超长（未结束标记）按字面透传，不无限缓冲
          if (buf.length - i <= MAX_PENDING_TAG_LEN) break;
          text += buf[i];
          i += 1;
          continue;
        }
        if (generic && generic !== "partial") {
          const literal = buf.slice(i, i + generic.length);
          if (generic.kind === "close") {
            const stackIdx = this.unknownStack.lastIndexOf(generic.name);
            if (stackIdx !== -1) {
              // 正常未知标记的闭标签：原样保留，弹出对应开启记录
              this.unknownStack.splice(stackIdx, 1);
              text += literal;
            } else if (this.dropUnknownOrphanClosers && !this.conservativePassthrough) {
              // 真正无开启记录的未知闭标签：仅最终边界清理（协议残渣）
            } else {
              text += literal;
            }
          } else if (generic.kind === "open") {
            text += literal;
            if (!this.conservativePassthrough) {
              if (this.unknownStack.length >= MAX_TAG_STACK_DEPTH) {
                // 深度上限：转入保守透传并停止本段激进清理（不丢正文）
                this.conservativePassthrough = true;
              } else {
                this.unknownStack.push(generic.name);
              }
            }
          } else {
            // 自闭合 / 注释 / CDATA / 声明：原样保留，不建立开启状态
            text += literal;
          }
          i += generic.length;
          continue;
        }
        if (this.dropUnknownOrphanClosers && !this.conservativePassthrough && buf.startsWith("</", i)) {
          const rest = buf.slice(i);
          const closeMatch = CLOSE_TAG_SHAPE.exec(rest);
          if (closeMatch) {
            // 词表外且不构成通用形状（理论上不可达，形状匹配已覆盖；保守兜底）
            i += closeMatch[0].length;
            continue;
          }
          if (!isFlush && PARTIAL_TAG_SHAPE.test(rest)) break; // 尾巴可能是跨 delta 的收尾标签，挂起
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
 * 用于历史消息重渲染等"全文已在手"的场景；转义（反斜杠保留）与代码保护
 * 规则与流式扫描一致。调用方是最终文本边界，孤儿闭标签形状规则开启。
 */
export function splitReservedTagSegments(
  content: string,
  tags: readonly string[],
): ReservedTagSegment[] {
  const scanner = new ReservedTagScanner(tags, { dropUnknownOrphanClosers: true });
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
