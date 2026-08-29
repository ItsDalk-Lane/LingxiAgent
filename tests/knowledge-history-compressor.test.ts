import { describe, expect, it } from "vitest";
import { compressHistoricalKnowledgeContextMessages } from "../core/knowledge-history-compressor.ts";

const PREFIX = "[KnowledgeContext]";
const END = "[/KnowledgeContext]";

function userMessage(content: string) {
  return { role: "user", content };
}

function assistantMessage(content: string) {
  return { role: "assistant", content };
}

function fullKnowledgeBlock(headers: string[]) {
  return [
    PREFIX,
    "Knowledge notebook evidence retrieved for the user's question (not part of the user's message).",
    `Evidence blocks (total budget 6000 tokens, retrieval mode: hybrid):`,
    ...headers.map((header, i) => `${header}\n证据正文第 ${i + 1} 段：${"很长的内容".repeat(40)}`),
    "Guidance (question-answer mode): Answer only from the evidence blocks above.",
    END,
  ].join("\n");
}

const HEADERS = [
  '[K1] notebook "研究" / source "论文" (sourceId: src_0001) / chunk ordinal 4 / heading: Intro',
  '[K2] notebook "资料" / source "报告" (sourceId: src_0002) / chunk ordinal 1 / page: 12',
];

describe("compressHistoricalKnowledgeContextMessages", () => {
  it("user 消息中的历史注入块替换为编号清单；正文不再出现", () => {
    const messages = [
      userMessage(fullKnowledgeBlock(HEADERS)),
      assistantMessage("根据证据回答。"),
      userMessage("追问"),
    ];
    const { messages: next, changed } = compressHistoricalKnowledgeContextMessages(messages);
    expect(changed).toBe(true);
    const compressed = (next[0] as any).content as string;
    expect(compressed).toContain(PREFIX);
    expect(compressed).toContain("full content omitted to save context");
    expect(compressed).toContain('- [K1] notebook "研究" / source "论文" (sourceId: src_0001) / chunk ordinal 4');
    expect(compressed).toContain('- [K2] notebook "资料" / source "报告" (sourceId: src_0002) / chunk ordinal 1');
    expect(compressed).toContain("`knowledge_read` tool");
    // 正文被压缩掉
    expect(compressed).not.toContain("证据正文第 1 段");
    expect(compressed).not.toContain("很长的内容");
    // assistant 消息与非注入 user 消息不动
    expect((next[1] as any).content).toBe("根据证据回答。");
    expect((next[2] as any).content).toBe("追问");
  });

  it("注入块前后的普通文本保留", () => {
    const messages = [
      userMessage(`前置说明\n\n${fullKnowledgeBlock(HEADERS)}\n\n后置问题`),
    ];
    const { messages: next } = compressHistoricalKnowledgeContextMessages(messages);
    const compressed = (next[0] as any).content as string;
    expect(compressed.startsWith("前置说明")).toBe(true);
    expect(compressed.endsWith("后置问题")).toBe(true);
  });

  it("旧格式（无 sourceId）的注入块退化为原样保留", () => {
    const legacy = [
      PREFIX,
      'Evidence blocks (total budget 6000 tokens):',
      '[K1] notebook "研究" / source "论文" / chunk ordinal 4',
      "证据正文。",
      END,
    ].join("\n");
    const messages = [userMessage(legacy)];
    const { changed } = compressHistoricalKnowledgeContextMessages(messages);
    expect(changed).toBe(false);
  });

  it("已压缩块幂等（二次压缩 no-op）", () => {
    const once = compressHistoricalKnowledgeContextMessages([userMessage(fullKnowledgeBlock(HEADERS))]);
    expect(once.changed).toBe(true);
    const twice = compressHistoricalKnowledgeContextMessages(once.messages);
    expect(twice.changed).toBe(false);
    expect(twice.messages).toBe(once.messages);
  });

  it("未闭合信封 fail-closed（残缺正文不泄漏）", () => {
    const broken = `${PREFIX}\n${HEADERS[0]}\n证据正文泄漏？`;
    const messages = [userMessage(broken)];
    const { messages: next, changed } = compressHistoricalKnowledgeContextMessages(messages);
    expect(changed).toBe(true);
    const compressed = (next[0] as any).content as string;
    expect(compressed).not.toContain("证据正文泄漏");
    expect(compressed).toContain(HEADERS[0].split(' / heading')[0]);
    expect(compressed).toContain(END);
  });

  it("content 为 blocks 数组的消息逐 block 压缩", () => {
    const messages = [
      {
        role: "user",
        content: [
          { type: "text", text: fullKnowledgeBlock(HEADERS) },
          { type: "text", text: "普通文本块" },
        ],
      },
    ];
    const { messages: next, changed } = compressHistoricalKnowledgeContextMessages(messages);
    expect(changed).toBe(true);
    const blocks = (next[0] as any).content as Array<{ text: string }>;
    expect(blocks[0].text).not.toContain("证据正文第 1 段");
    expect(blocks[1].text).toBe("普通文本块");
  });

  it("非数组输入原样返回", () => {
    expect(compressHistoricalKnowledgeContextMessages(null).changed).toBe(false);
    expect(compressHistoricalKnowledgeContextMessages("not array").changed).toBe(false);
    expect(compressHistoricalKnowledgeContextMessages([]).changed).toBe(false);
  });
});
