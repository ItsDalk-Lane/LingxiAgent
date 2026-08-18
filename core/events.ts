/**
 * MoodParser / ThinkTagParser — 内部保留协议标签的流式解析
 *
 * 契约（第二阶段收口）：<mood>/<pulse>/<reflect> 与 <think>/<thinking> 是保留协议，
 * 不是普通文本。无论出现在一次生成的什么位置（开头、中段、结尾），都必须被结构化；
 * 一次生成里允许出现多个同族块。需要字面量时由模型用 `\<tag>` 转义或放进
 * 行内代码 / 围栏代码块（扫描器对这两种位置一律按文本透传）。
 *
 * 两个解析器共享同一个扫描内核（ReservedTagScanner），只是把 token 映射成各自的
 * 事件名：
 *   MoodParser:      mood_start / mood_text / mood_end / text
 *   ThinkTagParser:  think_start / think_text / think_end / text
 */

import { INTERNAL_MOOD_TAGS } from "../shared/internal-mood-block.ts";
import { ReservedTagScanner } from "../shared/reserved-tag-stream.ts";

class ReservedTagParserBase {
  declare private scanner: ReservedTagScanner;
  declare private inTag: boolean;
  declare private justEnded: boolean;
  declare private readonly eventNames: { start: string; text: string; end: string };

  constructor(tags: readonly string[], eventNames: { start: string; text: string; end: string }) {
    this.scanner = new ReservedTagScanner(tags);
    this.eventNames = eventNames;
    this.inTag = false;
    this.justEnded = false;
  }

  /**
   * 喂入一段 streaming delta 文本，通过 emit 回调输出解析后的事件
   * @param {string} delta
   * @param {(evt: {type: string, data?: string}) => void} emit
   */
  feed(delta, emit) {
    for (const token of this.scanner.feed(delta)) this.handleToken(token, emit);
  }

  /** 冲刷缓冲：半截标签按字面文本定界；未闭合的块补发结束事件。 */
  flush(emit) {
    for (const token of this.scanner.flush()) this.handleToken(token, emit);
    if (this.inTag) {
      emit({ type: this.eventNames.end });
      this.inTag = false;
      this.justEnded = true;
    }
  }

  /**
   * 新的 assistant segment 边界（message_start(role=assistant)）：清空只属于本段
   * 生成的解析状态。调用方必须先 flush，否则挂起的半截标签会随缓冲一起被丢弃。
   */
  beginAssistantSegment() {
    this.scanner.reset();
    this.inTag = false;
    this.justEnded = false;
  }

  /** 整个 user turn 边界（turn_start / turn_end / abort）：turn 重置蕴含 segment 重置。 */
  reset() {
    this.beginAssistantSegment();
  }

  handleToken(token, emit) {
    if (token.type === "open") {
      emit({ type: this.eventNames.start });
      this.inTag = true;
      return;
    }
    if (token.type === "close") {
      emit({ type: this.eventNames.end });
      this.inTag = false;
      this.justEnded = true;
      return;
    }
    let text = token.text;
    // 块刚结束时，裁掉紧跟着的前导换行（块与正文之间的排版空行不进正文）
    if (!this.inTag && this.justEnded) {
      text = text.replace(/^\n+/, "");
      this.justEnded = false;
    }
    if (!text) return;
    emit({ type: this.inTag ? this.eventNames.text : "text", data: text });
  }
}

export class MoodParser extends ReservedTagParserBase {
  constructor() {
    super(INTERNAL_MOOD_TAGS, { start: "mood_start", text: "mood_text", end: "mood_end" });
  }
}

/**
 * ThinkTagParser — 拦截 <think>/<thinking> 标签（DeepSeek / Qwen / Kimi 等模型的文本内思考格式）
 *
 * 链在 MoodParser 之前（最外层），输出事件流：
 *   think_start / think_text { data } / think_end
 *   text { data } — 非 think 内容透传
 */
const THINK_TAGS = ["think", "thinking"];

export class ThinkTagParser extends ReservedTagParserBase {
  constructor() {
    super(THINK_TAGS, { start: "think_start", text: "think_text", end: "think_end" });
  }
}

/**
 * CardParser — 从 streaming text 中解析 <card ...>...</card> 标签
 *
 * 链在 MoodParser 的 text 输出之后，输出事件流：
 *   card_start { attrs: { type?, plugin, route, title? } }
 *   card_text { data }
 *   card_end
 *   text { data } — 非 card 内容透传
 */

/** 检查 buffer 末尾是否是 target 的前缀（1..target.length-1 个字符），返回匹配长度 */
function trailingPrefixLen(buffer, target) {
  const maxCheck = Math.min(buffer.length, target.length - 1);
  for (let len = maxCheck; len >= 1; len--) {
    if (buffer.endsWith(target.slice(0, len))) return len;
  }
  return 0;
}

const CARD_ATTR_RE = /(\w+)="([^"]*)"/g;

export class CardParser {
  declare _attrs: any;
  declare buffer: any;
  declare inCard: any;
  constructor() {
    this.inCard = false;
    this.buffer = "";
    this._attrs = null;
  }

  feed(delta, emit) {
    this.buffer += delta;
    this._drain(emit);
  }

  flush(emit) {
    if (this.buffer) {
      if (this.inCard) {
        emit({ type: "card_text", data: this.buffer });
      } else {
        emit({ type: "text", data: this.buffer });
      }
      this.buffer = "";
    }
    if (this.inCard) {
      emit({ type: "card_end" });
      this.inCard = false;
      this._attrs = null;
    }
  }

  reset() {
    this.inCard = false;
    this.buffer = "";
    this._attrs = null;
  }

  _parseAttrs(tag) {
    const attrs = {};
    let m;
    CARD_ATTR_RE.lastIndex = 0;
    while ((m = CARD_ATTR_RE.exec(tag)) !== null) {
      attrs[m[1]] = m[2];
    }
    return attrs;
  }

  _findCardOpen() {
    // Find <card followed by space or > (word boundary — excludes <cardiac etc.)
    let searchFrom = 0;
    while (searchFrom < this.buffer.length) {
      const idx = this.buffer.indexOf("<card", searchFrom);
      if (idx === -1) return -1;
      const after = this.buffer[idx + 5];
      if (after === undefined || after === " " || after === ">" || after === "\n" || after === "\t") return idx;
      searchFrom = idx + 1;
    }
    return -1;
  }

  _drain(emit) {
    while (this.buffer.length > 0) {
      if (!this.inCard) {
        // Look for complete opening tag <card ... > (with word boundary)
        const openIdx = this._findCardOpen();
        if (openIdx !== -1) {
          // Check if the full opening tag is present (find closing >)
          const closeAngle = this.buffer.indexOf(">", openIdx);
          if (closeAngle !== -1) {
            const before = this.buffer.slice(0, openIdx);
            if (before) emit({ type: "text", data: before });
            const openTag = this.buffer.slice(openIdx, closeAngle + 1);
            this._attrs = this._parseAttrs(openTag);
            emit({ type: "card_start", attrs: this._attrs });
            this.inCard = true;
            this.buffer = this.buffer.slice(closeAngle + 1);
            continue;
          }
          // Have <card but no > yet — hold from <card onward
          const before = this.buffer.slice(0, openIdx);
          if (before) emit({ type: "text", data: before });
          this.buffer = this.buffer.slice(openIdx);
          break;
        }
        // Check trailing prefix for partial <card
        const holdLen = trailingPrefixLen(this.buffer, "<card");
        if (holdLen > 0) {
          const safe = this.buffer.slice(0, -holdLen);
          if (safe) emit({ type: "text", data: safe });
          this.buffer = this.buffer.slice(-holdLen);
          break;
        }
        emit({ type: "text", data: this.buffer });
        this.buffer = "";
      } else {
        // Inside card — look for </card>
        const closeTag = "</card>";
        const idx = this.buffer.indexOf(closeTag);
        if (idx !== -1) {
          const content = this.buffer.slice(0, idx);
          if (content) emit({ type: "card_text", data: content });
          emit({ type: "card_end" });
          this.inCard = false;
          this._attrs = null;
          this.buffer = this.buffer.slice(idx + closeTag.length);
          continue;
        }
        const holdLen = trailingPrefixLen(this.buffer, closeTag);
        if (holdLen > 0) {
          const safe = this.buffer.slice(0, -holdLen);
          if (safe) emit({ type: "card_text", data: safe });
          this.buffer = this.buffer.slice(-holdLen);
          break;
        }
        emit({ type: "card_text", data: this.buffer });
        this.buffer = "";
      }
    }
  }
}
