/**
 * 计划文件路径契约测试：会话旁 <sessionId>.plan.md 的派生与判定。
 * 三处消费方（只读豁免 / 收工闸门 / 压缩保护）共用这一条约定。
 */
import { describe, expect, it } from "vitest";
import path from "path";
import {
  PLAN_FILE_SUFFIX,
  isPlanFilePath,
  isPlanFileWrite,
  planFilePathForSession,
} from "../lib/plan-mode/plan-file.ts";

const SESSION = "/home/u/.lingxi/agents/main/sessions/abc123.jsonl";

describe("plan-file 路径契约", () => {
  it("由会话 jsonl 派生计划文件路径", () => {
    expect(planFilePathForSession(SESSION)).toBe(
      path.join("/home/u/.lingxi/agents/main/sessions", `abc123${PLAN_FILE_SUFFIX}`),
    );
  });

  it("非字符串/空串/无 basename 返回 null", () => {
    expect(planFilePathForSession(null)).toBeNull();
    expect(planFilePathForSession("")).toBeNull();
    expect(planFilePathForSession("   ")).toBeNull();
    expect(planFilePathForSession(42)).toBeNull();
  });

  it("非 .jsonl 后缀的会话路径也按 basename 派生", () => {
    expect(planFilePathForSession("/tmp/sess")).toBe(`/tmp/sess${PLAN_FILE_SUFFIX}`);
  });

  it("isPlanFilePath 只认解析后全等", () => {
    const plan = planFilePathForSession(SESSION)!;
    expect(isPlanFilePath(SESSION, plan)).toBe(true);
    expect(isPlanFilePath(SESSION, `${plan}.bak`)).toBe(false);
    expect(isPlanFilePath(SESSION, "/home/u/.lingxi/agents/main/sessions/other.plan.md")).toBe(false);
    // 别的会话的同名约定文件不行
    expect(isPlanFilePath("/x/y.jsonl", plan)).toBe(false);
    expect(isPlanFilePath(null, plan)).toBe(false);
    expect(isPlanFilePath(SESSION, "")).toBe(false);
  });

  it("isPlanFileWrite 只认 write/edit 且路径命中", () => {
    const plan = planFilePathForSession(SESSION)!;
    expect(isPlanFileWrite("write", { path: plan }, SESSION)).toBe(true);
    expect(isPlanFileWrite("edit", { path: plan }, SESSION)).toBe(true);
    expect(isPlanFileWrite("read", { path: plan }, SESSION)).toBe(false);
    expect(isPlanFileWrite("write", { path: "/tmp/x.md" }, SESSION)).toBe(false);
    expect(isPlanFileWrite("write", {}, SESSION)).toBe(false);
    expect(isPlanFileWrite("write", { path: plan }, null)).toBe(false);
  });
});
