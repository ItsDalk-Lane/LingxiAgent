/**
 * MoodParser / ThinkTagParser — 内部保留协议标签的流式解析
 *
 * 契约（第二阶段收口）：<mood>/<pulse>/<reflect> 与 <think>/<thinking> 是保留协议，
 * 不是普通文本。无论出现在一次生成的什么位置（开头、中段、结尾），都必须被结构化；
 * 一次生成里允许出现多个同族块。需要字面量时由模型用 `\<tag>` 转义或放进
 * 行内代码 / 围栏代码块（扫描器对这两种位置一律按文本透传）。
 *
 * F8/P6.3：转义反斜杠在解析链上保留（`\<tag>` → 文本 `\<tag>`，任何中间层都
 * 不得消费——链尾吞孤儿闭标签时受保护字面量必须安全），显示层
 * （StreamingMarkdownContent → stripTagEscapes）一次性消费。
 *
 * 两个解析器共享同一个扫描内核（ReservedTagScanner），只是把 token 映射成各自的
 * 事件名：
 *   MoodParser:      mood_start / mood_text / mood_end / text
 *   ThinkTagParser:  think_start / think_text / think_end / text
 */

import { INTERNAL_MOOD_TAGS } from "../shared/internal-mood-block.ts";
import { ReservedTagScanner, type ReservedTagScannerOptions } from "../shared/reserved-tag-stream.ts";

class ReservedTagParserBase {
  declare private scanner: ReservedTagScanner;
  declare private inTag: boolean;
  declare private justEnded: boolean;
  declare private readonly eventNames: { start: string; text: string; end: string };

  constructor(
    tags: readonly string[],
    eventNames: { start: string; text: string; end: string },
    scannerOptions: ReservedTagScannerOptions = {},
  ) {
    this.scanner = new ReservedTagScanner(tags, scannerOptions);
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
    // MoodParser 是解析链的链尾（输出即可见正文）：孤儿闭标签形状规则在此兜底。
    super(INTERNAL_MOOD_TAGS, { start: "mood_start", text: "mood_text", end: "mood_end" }, {
      dropUnknownOrphanClosers: true,
    });
  }
}

/**
 * ThinkTagParser — 拦截 <think>/<thinking> 标签（DeepSeek / Qwen / Kimi 等模型的文本内思考格式）
 *
 * 链在 MoodParser 之前（最外层），输出事件流：
 *   think_start / think_text { data } / think_end
 *   text { data } — 非 think 内容透传
 */
// mm:think 是 MiniMax M3 的思考方言：成对出现时结构化为思考块；API 只漏出孤立
// 闭标签时由 ReservedTagScanner 的形状规则兜底吞掉（见 shared/reserved-tag-stream.ts）。
const THINK_TAGS = ["think", "thinking", "mm:think"];

export class ThinkTagParser extends ReservedTagParserBase {
  constructor() {
    super(THINK_TAGS, { start: "think_start", text: "think_text", end: "think_end" });
  }
}

