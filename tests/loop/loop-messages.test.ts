import { describe, it, expect } from "vitest";
import {
  LOOP_TURN_MESSAGE_TYPE,
  LOOP_NOTICE_MESSAGE_TYPE,
  buildLoopKickoffMessage,
  buildLoopWakeupMessage,
  buildLoopNoticeMessage,
  buildLoopInterludeBlock,
} from "../../lib/loop/loop-messages.ts";

const loop = {
  key: "sid-a",
  prompt: "watch the pipeline",
  turnCount: 2,
  limits: { maxTurns: 50, maxConsecutiveFailures: 3, minDelaySec: 60, guardedMinDelaySec: 1200, fallbackDelaySec: 1200 },
};

describe("loop messages", () => {
  it("kickoff message carries the task, the turn contract, and the budget", () => {
    const msg = buildLoopKickoffMessage(loop);
    expect(msg.customType).toBe(LOOP_TURN_MESSAGE_TYPE);
    expect(msg.display).toBe(false);
    expect(msg.content).toContain("watch the pipeline");
    expect(msg.content).toContain("loop_control");
    expect(msg.content).toContain("50");
    expect(msg.details).toMatchObject({ schemaVersion: 1, kind: "kickoff" });
  });

  it("wakeup message carries reason and progress", () => {
    const msg = buildLoopWakeupMessage(loop, "check remote pipeline status");
    expect(msg.customType).toBe(LOOP_TURN_MESSAGE_TYPE);
    expect(msg.content).toContain("check remote pipeline status");
    expect(msg.content).toContain("2/50");
    expect(msg.details).toMatchObject({ schemaVersion: 1, kind: "wakeup" });
  });

  it("notice message uses the notice type and shows text verbatim", () => {
    const msg = buildLoopNoticeMessage("循环已暂停");
    expect(msg.customType).toBe(LOOP_NOTICE_MESSAGE_TYPE);
    expect(msg.content).toContain("循环已暂停");
  });

  it("kickoff details carry structured turnCount/maxTurns", () => {
    const msg = buildLoopKickoffMessage(loop);
    expect(msg.details.turnCount).toBe(2);
    expect(msg.details.maxTurns).toBe(50);
  });

  it("wakeup details carry structured turnCount/maxTurns", () => {
    const msg = buildLoopWakeupMessage(loop, "check remote pipeline status");
    expect(msg.details.turnCount).toBe(2);
    expect(msg.details.maxTurns).toBe(50);
  });
});

describe("buildLoopInterludeBlock", () => {
  it("turns a new kickoff message (details.prompt) into a loop interlude bubble", () => {
    const msg = { ...buildLoopKickoffMessage(loop), id: "abc123" };
    const block = buildLoopInterludeBlock(msg);
    expect(block).not.toBeNull();
    expect(block.type).toBe("interlude");
    expect(block.variant).toBe("loop");
    expect(block.sourceKind).toBe("loop");
    expect(block.text).toBe("🔁 循环任务已启动");
    expect(block.detailMarkdown).toBe("watch the pipeline");
    expect(block.id).toBe("loop:abc123");
  });

  it("falls back to parsing Task: line from content for legacy kickoff messages (no details.prompt)", () => {
    // 模拟改动前生成的旧 kickoff（details 没有 prompt），正是用户那个会话的数据形态
    const legacy = {
      customType: LOOP_TURN_MESSAGE_TYPE,
      display: false,
      id: "2c5f6576",
      content: `<hana-loop kind="kickoff">\nThis session is now in recurring-loop mode.\nTask: 把上述四层全部补上，第四层平时是隐藏的设置一个开启的开关\n\nEach loop turn: do the work now.\n</hana-loop>`,
      details: { schemaVersion: 1, kind: "kickoff" },
    };
    const block = buildLoopInterludeBlock(legacy);
    expect(block).not.toBeNull();
    expect(block.text).toBe("🔁 循环任务已启动");
    expect(block.detailMarkdown).toBe("把上述四层全部补上，第四层平时是隐藏的设置一个开启的开关");
    expect(block.id).toBe("loop:2c5f6576");
  });

  it("turns a wakeup message into a continue bubble with reason in detail", () => {
    const msg = { ...buildLoopWakeupMessage(loop, "loop resumed"), id: "w1" };
    const block = buildLoopInterludeBlock(msg);
    expect(block).not.toBeNull();
    expect(block.text).toBe("🔁 循环任务继续");
    expect(block.detailMarkdown).toContain("watch the pipeline");
    expect(block.detailMarkdown).toContain("loop resumed");
  });

  it("shows notice content verbatim as the bubble text", () => {
    const msg = { ...buildLoopNoticeMessage("（会话已重置，循环终止。）"), id: "n1" };
    const block = buildLoopInterludeBlock(msg);
    expect(block).not.toBeNull();
    expect(block.text).toBe("（会话已重置，循环终止。）");
    expect(block.detailMarkdown).toBeUndefined();
  });

  it("produces a stable id derived from the message entry id", () => {
    const msg = { ...buildLoopKickoffMessage(loop), id: "xyz" };
    expect(buildLoopInterludeBlock(msg).id).toBe(buildLoopInterludeBlock(msg).id);
    expect(buildLoopInterludeBlock(msg).id).toBe("loop:xyz");
  });

  it("returns null for non-loop custom messages", () => {
    expect(buildLoopInterludeBlock({ customType: "deferred-result", display: false, details: {} })).toBeNull();
    expect(buildLoopInterludeBlock({ customType: "turn_input_presentation", display: false, details: {} })).toBeNull();
  });

  it("returns null when a kickoff has no prompt anywhere to recover", () => {
    expect(buildLoopInterludeBlock({ customType: LOOP_TURN_MESSAGE_TYPE, content: "no task here", details: { kind: "kickoff" } })).toBeNull();
    expect(buildLoopInterludeBlock({ customType: LOOP_NOTICE_MESSAGE_TYPE, content: "   ", details: { kind: "notice" } })).toBeNull();
  });

  it("reads turnCount/maxTurns from details into the interlude block", () => {
    const kickoff = buildLoopInterludeBlock({ ...buildLoopKickoffMessage(loop), id: "k1" });
    expect(kickoff.turnCount).toBe(2);
    expect(kickoff.maxTurns).toBe(50);

    const wakeup = buildLoopInterludeBlock({ ...buildLoopWakeupMessage(loop, "check now"), id: "w1" });
    expect(wakeup.turnCount).toBe(2);
    expect(wakeup.maxTurns).toBe(50);
  });

  it("falls back to the Progress line in content for legacy wakeups (no details.turnCount)", () => {
    // 旧 wakeup：details 无 turnCount/maxTurns，但协议正文带 "Progress: loop turn X/Y" 行
    const legacy = {
      customType: LOOP_TURN_MESSAGE_TYPE,
      display: false,
      id: "legacy-w",
      content: `<hana-loop kind="wakeup">\nScheduled wakeup fired. Reason: check\nLoop task: watch the pipeline\nProgress: loop turn 3/50.\n</hana-loop>`,
      details: { schemaVersion: 1, kind: "wakeup", prompt: "watch the pipeline" },
    };
    const block = buildLoopInterludeBlock(legacy);
    expect(block).not.toBeNull();
    expect(block.turnCount).toBe(3);
    expect(block.maxTurns).toBe(50);
  });

  it("omits turnCount/maxTurns for legacy kickoffs whose content has no Progress line", () => {
    const legacy = {
      customType: LOOP_TURN_MESSAGE_TYPE,
      display: false,
      id: "legacy-k",
      content: `<hana-loop kind="kickoff">\nThis session is now in recurring-loop mode.\nTask: watch the pipeline\n</hana-loop>`,
      details: { schemaVersion: 1, kind: "kickoff" },
    };
    const block = buildLoopInterludeBlock(legacy);
    expect(block).not.toBeNull();
    expect(block).not.toHaveProperty("turnCount");
    expect(block).not.toHaveProperty("maxTurns");
  });
});
