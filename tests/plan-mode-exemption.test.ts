/**
 * 计划模式只读豁免测试：read_only 档下只放行本会话约定计划文件的写，
 * 其余照旧全拒；无会话路径时豁免不命中（fail-closed）。
 */
import { describe, expect, it } from "vitest";
import { classifySessionPermission } from "../core/session-permission-mode.ts";
import { planFilePathForSession } from "../lib/plan-mode/plan-file.ts";

const SESSION = "/tmp/agent/sessions/sess-1.jsonl";
const PLAN = planFilePathForSession(SESSION)!;

function classify(mode: string, toolName: string, params: any, context: any = {}) {
  return classifySessionPermission({ mode, toolName, params, context });
}

describe("计划模式（read_only）计划文件豁免", () => {
  it("写本会话 plan.md 放行（write 与 edit）", () => {
    expect(classify("read_only", "write", { path: PLAN }, { sessionPath: SESSION }).action).toBe("allow");
    expect(classify("read_only", "edit", { path: PLAN }, { sessionPath: SESSION }).action).toBe("allow");
  });

  it("写别的文件照旧拒绝", () => {
    const decision = classify("read_only", "write", { path: "/tmp/other.md" }, { sessionPath: SESSION });
    expect(decision.action).toBe("deny");
  });

  it("别的会话的 plan.md 不放行", () => {
    const foreign = planFilePathForSession("/tmp/agent/sessions/sess-2.jsonl")!;
    expect(classify("read_only", "write", { path: foreign }, { sessionPath: SESSION }).action).toBe("deny");
  });

  it("无 sessionPath 上下文时不命中（fail-closed）", () => {
    expect(classify("read_only", "write", { path: PLAN }, {}).action).toBe("deny");
    expect(classify("read_only", "write", { path: PLAN }, { sessionPath: null }).action).toBe("deny");
  });

  it("read 工具不受影响，仍全模式直通", () => {
    expect(classify("read_only", "read", { path: "/tmp/x" }, { sessionPath: SESSION }).action).toBe("allow");
  });

  it("operate/auto 档行为不因豁免改变", () => {
    expect(classify("operate", "write", { path: "/tmp/other.md" }, { sessionPath: SESSION }).action).toBe("allow");
    expect(classify("auto", "write", { path: "/tmp/other.md" }, { sessionPath: SESSION }).action).toBe("allow");
  });

  it("ask 档不因豁免改变（豁免只属于 read_only）", () => {
    const decision = classify("ask", "write", { path: PLAN }, { sessionPath: SESSION });
    expect(decision.action).not.toBe("allow");
  });
});
