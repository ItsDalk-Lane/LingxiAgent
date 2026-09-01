import { describe, expect, it, vi } from "vitest";
import {
  MODEL_CALL_REFERENCE_RECORD_TYPE,
  noteModelCallMessageIdentity,
  persistModelCallReferenceForMessage,
} from "../lib/llm/model-call-correlation.ts";
import { collectModelCallReferencesBySourceIndex } from "../core/message-utils.ts";

describe("助手消息与模型调用持久关联", () => {
  it("在助手消息落盘前写入不含正文的隐藏关联条目", () => {
    const message = { role: "assistant", content: [{ type: "text", text: "完成" }] };
    noteModelCallMessageIdentity(message, {
      modelCallId: "mc_exact",
      traceId: "mt_exact",
      parentCallId: null,
    });
    const appendCustomEntry = vi.fn();

    expect(persistModelCallReferenceForMessage({ appendCustomEntry }, message)).toEqual({
      modelCallId: "mc_exact",
      traceId: "mt_exact",
      parentCallId: null,
    });
    expect(appendCustomEntry).toHaveBeenCalledWith(MODEL_CALL_REFERENCE_RECORD_TYPE, {
      schemaVersion: 1,
      modelCallId: "mc_exact",
      traceId: "mt_exact",
      parentCallId: null,
    });
    expect(JSON.stringify(appendCustomEntry.mock.calls)).not.toContain("完成");
  });

  it("隐藏关联只绑定其后第一条助手消息，遇到用户消息即失效", () => {
    const ref = (id: string) => ({
      role: "custom",
      customType: MODEL_CALL_REFERENCE_RECORD_TYPE,
      display: false,
      data: { schemaVersion: 1, modelCallId: id, traceId: `mt_${id}`, parentCallId: null },
    });
    const messages = [
      ref("mc_1"),
      { role: "assistant", content: "第一条" },
      { role: "assistant", content: "第二条" },
      ref("mc_stale"),
      { role: "user", content: "新一轮" },
      { role: "assistant", content: "第三条" },
    ];

    expect([...collectModelCallReferencesBySourceIndex(messages).entries()]).toEqual([
      [1, { modelCallId: "mc_1", traceId: "mt_mc_1", parentCallId: null }],
    ]);
  });
});
