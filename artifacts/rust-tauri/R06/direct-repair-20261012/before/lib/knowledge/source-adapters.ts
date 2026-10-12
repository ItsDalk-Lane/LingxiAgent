import { JSDOM } from "jsdom";

export type KnowledgeBlockDraft = {
  ordinal: number;
  text: string;
  locatorType: "text" | "markdown" | "pdf" | "html";
  locator: Record<string, unknown>;
};

export type ParsedKnowledgeSnapshot = {
  status: "ready" | "needs_ocr";
  warnings: string[];
  semanticText: string;
  blocks: KnowledgeBlockDraft[];
};

function decodeUtf8(bytes: Buffer): string {
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes).replace(/^\uFEFF/u, "");
  } catch {
    throw new Error("invalid_utf8");
  }
}

function normalizedLines(text: string) {
  const normalized = text.replace(/\r\n?/gu, "\n");
  const lines = normalized.split("\n");
  let cursor = 0;
  return lines.map((raw, index) => {
    const start = cursor;
    cursor += raw.length + (index < lines.length - 1 ? 1 : 0);
    return { raw, lineNumber: index + 1, start };
  });
}

function parsePlainText(bytes: Buffer): ParsedKnowledgeSnapshot {
  const blocks: KnowledgeBlockDraft[] = [];
  for (const line of normalizedLines(decodeUtf8(bytes))) {
    const leading = line.raw.length - line.raw.trimStart().length;
    const text = line.raw.trim();
    if (!text) continue;
    const charStart = line.start + leading;
    blocks.push({
      ordinal: blocks.length,
      text,
      locatorType: "text",
      locator: {
        lineStart: line.lineNumber,
        lineEnd: line.lineNumber,
        charStart,
        charEnd: charStart + text.length,
      },
    });
  }
  return { status: "ready", warnings: [], semanticText: blocks.map((block) => block.text).join("\n"), blocks };
}

function parseMarkdown(bytes: Buffer): ParsedKnowledgeSnapshot {
  const blocks: KnowledgeBlockDraft[] = [];
  const headingPath: string[] = [];
  for (const line of normalizedLines(decodeUtf8(bytes))) {
    const trimmed = line.raw.trim();
    if (!trimmed) continue;
    const heading = trimmed.match(/^(#{1,6})\s+(.+?)\s*#*$/u);
    let text = trimmed;
    let startInLine = line.raw.indexOf(trimmed);
    if (heading) {
      const level = heading[1].length;
      text = heading[2].trim();
      headingPath.splice(level - 1);
      headingPath[level - 1] = text;
      headingPath.splice(level);
      startInLine = line.raw.indexOf(text, startInLine + heading[1].length);
    }
    const charStart = line.start + Math.max(0, startInLine);
    blocks.push({
      ordinal: blocks.length,
      text,
      locatorType: "markdown",
      locator: {
        headingPath: [...headingPath],
        lineStart: line.lineNumber,
        lineEnd: line.lineNumber,
        charStart,
        charEnd: charStart + text.length,
      },
    });
  }
  return { status: "ready", warnings: [], semanticText: blocks.map((block) => block.text).join("\n"), blocks };
}

const HTML_BLOCK_SELECTOR = "h1,h2,h3,h4,h5,h6,p,li,blockquote,pre,td,th";

function structuralPath(element: Element): string {
  const parts: string[] = [];
  let current: Element | null = element;
  while (current) {
    const name = current.localName.toLowerCase();
    let ordinal = 1;
    let previous = current.previousElementSibling;
    while (previous) {
      if (previous.localName.toLowerCase() === name) ordinal += 1;
      previous = previous.previousElementSibling;
    }
    parts.unshift(`${name}:nth-of-type(${ordinal})`);
    current = current.parentElement;
  }
  return parts.join(" > ");
}

function parseHtml(bytes: Buffer): ParsedKnowledgeSnapshot {
  const dom = new JSDOM(decodeUtf8(bytes), { contentType: "text/html" });
  try {
    const document = dom.window.document;
    document.querySelectorAll("script,style,noscript,template").forEach((node) => node.remove());
    const headingPath: string[] = [];
    const blocks: KnowledgeBlockDraft[] = [];
    for (const element of document.querySelectorAll(HTML_BLOCK_SELECTOR)) {
      // 嵌套的块级元素由更具体的后代独立定位，父节点不重复收录正文。
      if (element.querySelector(HTML_BLOCK_SELECTOR)) continue;
      const text = (element.textContent || "").replace(/\s+/gu, " ").trim();
      if (!text) continue;
      const heading = element.localName.match(/^h([1-6])$/u);
      if (heading) {
        const level = Number(heading[1]);
        headingPath.splice(level - 1);
        headingPath[level - 1] = text;
        headingPath.splice(level);
      }
      blocks.push({
        ordinal: blocks.length,
        text,
        locatorType: "html",
        locator: {
          structuralPath: structuralPath(element),
          headingPath: [...headingPath],
          characterRange: { start: 0, end: text.length },
        },
      });
    }
    return { status: "ready", warnings: [], semanticText: blocks.map((block) => block.text).join("\n"), blocks };
  } finally {
    dom.window.close();
  }
}

function finiteNumber(value: unknown, fallback = 0): number {
  const number = Number(value);
  return Number.isFinite(number) ? number : fallback;
}

async function parsePdf(bytes: Buffer): Promise<ParsedKnowledgeSnapshot> {
  const data = new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const { getDocumentProxy } = await import("unpdf");
  const pdf = await getDocumentProxy(data);
  const blocks: KnowledgeBlockDraft[] = [];
  let itemCount = 0;
  try {
    for (let pageNumber = 1; pageNumber <= pdf.numPages; pageNumber += 1) {
      const page = await pdf.getPage(pageNumber);
      const content = await page.getTextContent();
      const items = Array.isArray(content.items) ? content.items : [];
      let pageCursor = 0;
      let lineParts: string[] = [];
      let lineBoxes: Array<Record<string, unknown>> = [];
      let lineItemStart = 0;

      const finishLine = (itemEnd: number) => {
        const text = lineParts.join(" ").replace(/\s+/gu, " ").trim();
        if (text) {
          const charStart = pageCursor;
          blocks.push({
            ordinal: blocks.length,
            text,
            locatorType: "pdf",
            locator: {
              page: pageNumber,
              pageCharStart: charStart,
              pageCharEnd: charStart + text.length,
              itemStart: lineItemStart,
              itemEnd,
              boxes: lineBoxes,
            },
          });
          pageCursor += text.length + 1;
        }
        lineParts = [];
        lineBoxes = [];
      };

      for (let index = 0; index < items.length; index += 1) {
        const item: any = items[index];
        if (typeof item?.str !== "string") continue;
        itemCount += 1;
        const text = item.str.replace(/\s+/gu, " ").trim();
        if (lineParts.length === 0) lineItemStart = index;
        if (text) {
          lineParts.push(text);
          const transform = Array.isArray(item.transform) ? item.transform : [];
          lineBoxes.push({
            x: finiteNumber(transform[4]),
            y: finiteNumber(transform[5]),
            width: finiteNumber(item.width),
            height: finiteNumber(item.height),
            fontSize: finiteNumber(item.fontSize, Math.abs(finiteNumber(transform[3]))),
            dir: typeof item.dir === "string" ? item.dir : "ltr",
          });
        }
        if (item.hasEOL) finishLine(index + 1);
      }
      if (lineParts.length > 0) finishLine(items.length);
    }
  } finally {
    await pdf.destroy?.();
  }

  if (itemCount === 0 || blocks.length === 0) {
    return { status: "needs_ocr", warnings: ["needs_ocr"], semanticText: "", blocks: [] };
  }
  return { status: "ready", warnings: [], semanticText: blocks.map((block) => block.text).join("\n"), blocks };
}

export async function parseCitationGradeSnapshot(input: {
  mimeType: string;
  bytes: Buffer;
}): Promise<ParsedKnowledgeSnapshot> {
  switch (input.mimeType) {
    case "text/plain":
      return parsePlainText(input.bytes);
    case "text/markdown":
      return parseMarkdown(input.bytes);
    case "text/html":
      return parseHtml(input.bytes);
    case "application/pdf":
      return parsePdf(input.bytes);
    default:
      throw new Error("unsupported_citation_format");
  }
}
