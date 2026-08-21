/**
 * ModelTraceScope 单元验证 — Phase 4 Trace Contract 的第一性原理（任务书
 * §九～§二十三/§四十九～§五十一/§五十三～§五十五）。
 *
 * 覆盖：
 *   - resolveModelTraceContext 三级优先：explicit > trace scope > singleton；
 *   - traceId 恒非空；parentCallId 无事实 → null（不猜）；
 *   - AsyncLocalStorage 并发隔离与异步链传播；
 *   - runWithNewModelTrace 的 detach 语义（覆盖外层 scope）；
 *   - runWithoutModelTrace 显式脱离；
 *   - 工具子 scope：causalParentCallId 冻结快照、并行分支互不覆盖（§三十一）；
 *   - noteAgentStreamCallStarted 只推进本链 lastCallId，子 scope 不受影响；
 *   - origin 有限枚举、refs 小型安全（键数/长度上限）。
 */
import { describe, it, expect } from "vitest";
import {
  MODEL_TRACE_ORIGINS,
  currentModelTraceScope,
  isModelTraceOrigin,
  noteAgentStreamCallStarted,
  resolveModelTraceContext,
  runToolExecutionWithModelTrace,
  runWithModelTraceRoot,
  runWithModelTrace,
  runWithNewModelTrace,
  runWithoutModelTrace,
} from "../lib/llm/model-trace-scope.ts";

async function tick() {
  await new Promise((resolve) => setTimeout(resolve, 5));
}

describe("resolveModelTraceContext — 三级优先（§二十一）", () => {
  it("无 scope 无 explicit → singleton：traceId 非空、parentCallId=null", () => {
    const resolved = resolveModelTraceContext();
    expect(resolved.traceId).toMatch(/^mt_/);
    expect(resolved.parentCallId).toBeNull();
    expect(resolved.source).toBe("singleton");
  });

  it("两次独立 unscoped 解析 → 各自 singleton（独立任务不合并，§五十二）", () => {
    const a = resolveModelTraceContext();
    const b = resolveModelTraceContext();
    expect(a.traceId).not.toBe(b.traceId);
  });

  it("explicit traceId 优先于 scope", () => {
    const resolved = runWithNewModelTrace({ origin: "user_turn" }, () =>
      resolveModelTraceContext({ traceId: "mt_explicit", parentCallId: "mc_parent" }));
    expect(resolved).toMatchObject({
      traceId: "mt_explicit",
      parentCallId: "mc_parent",
      source: "explicit",
    });
  });

  it("scope 内解析：traceId 继承、parent=lastCallId", () => {
    const resolved = runWithNewModelTrace({ origin: "user_turn" }, () => {
      noteAgentStreamCallStarted("mc_c1");
      return resolveModelTraceContext();
    });
    expect(resolved.source).toBe("trace_scope");
    expect(resolved.parentCallId).toBe("mc_c1");
    expect(resolved.origin).toBe("user_turn");
  });

  it("scope 内首个调用 parent=null（trace 根，§十四）", () => {
    const resolved = runWithNewModelTrace({ origin: "automation" }, () =>
      resolveModelTraceContext());
    expect(resolved.parentCallId).toBeNull();
  });

  it("explicit parent 指向自己 → 归 null（禁自环，§五十九）", () => {
    const resolved = resolveModelTraceContext({ traceId: "mt_x", parentCallId: "mt_x" });
    expect(resolved.parentCallId).toBeNull();
  });
});

describe("并发隔离与异步传播（§五十二）", () => {
  it("两个并行任务链的 scope 互不可见", async () => {
    const seen: string[] = [];
    const task = (marker: string) => runWithNewModelTrace({ origin: "user_turn" }, async () => {
      await tick();
      seen.push(`${marker}:${currentModelTraceScope()?.traceId === marker ? "own" : "wrong"}`);
    });
    // 先手动建立两个已知 traceId 的 scope：用 refs 传 marker，traceId 比对用 map
    const ids: string[] = [];
    const taskWithId = () => runWithNewModelTrace({ origin: "user_turn" }, async () => {
      const scope = currentModelTraceScope()!;
      ids.push(scope.traceId);
      await tick();
      // tick 之后读到的必须仍是自己的 scope
      return currentModelTraceScope()?.traceId;
    });
    const [a, b] = await Promise.all([taskWithId(), taskWithId()]);
    expect(ids[0]).not.toBe(ids[1]);
    expect(a).toBe(ids[0]);
    expect(b).toBe(ids[1]);
    expect(seen).toHaveLength(0);
  });

  it("异步 continuation 继承 scope（setTimeout 回调内仍可读）", async () => {
    const result = await runWithNewModelTrace({ origin: "background" }, async () => {
      await new Promise<void>((resolve) => setTimeout(resolve, 5));
      return currentModelTraceScope()?.origin ?? null;
    });
    expect(result).toBe("background");
  });
});

describe("detach 语义（§五十/§五十一）", () => {
  it("runWithNewModelTrace 覆盖外层 scope（内层看不到外层 trace）", () => {
    const outer = runWithNewModelTrace({ origin: "user_turn" }, () => {
      const outerScope = currentModelTraceScope()!;
      const inner = runWithNewModelTrace({ origin: "automation" }, () => ({
        traceId: currentModelTraceScope()!.traceId,
        origin: currentModelTraceScope()!.origin,
      }));
      return { outerScope, inner };
    });
    expect(outer.inner.traceId).not.toBe(outer.outerScope.traceId);
    expect(outer.inner.origin).toBe("automation");
  });

  it("runWithoutModelTrace：内层解析回 singleton", () => {
    const resolved = runWithNewModelTrace({ origin: "user_turn" }, () =>
      runWithoutModelTrace(() => resolveModelTraceContext()));
    expect(resolved.source).toBe("singleton");
    expect(resolved.parentCallId).toBeNull();
  });

  it("runWithModelTraceRoot：外层有 scope 原样继承，无 scope 铸新根", () => {
    const outer = runWithNewModelTrace({ origin: "user_turn" }, () => {
      const before = currentModelTraceScope()!;
      const inherited = runWithModelTraceRoot({ origin: "media" }, () =>
        currentModelTraceScope()!);
      return { before, inherited };
    });
    expect(outer.inherited).toBe(outer.before); // 同一对象：原样继承

    const fresh = runWithModelTraceRoot({ origin: "media" }, () => currentModelTraceScope()!);
    expect(fresh.origin).toBe("media");
    expect(fresh.traceId).toMatch(/^mt_/);
  });
});

describe("工具子 scope（§三十一/§三十二）", () => {
  it("进入工具时快照 lastCallId 为 causalParentCallId；并行分支互不覆盖", async () => {
    const parents: (string | null)[] = [];
    await runWithNewModelTrace({ origin: "user_turn" }, async () => {
      noteAgentStreamCallStarted("mc_c1"); // Chat C1 产生了 toolCalls
      const tool = async (label: string) => {
        await tick();
        parents.push(currentModelTraceScope()?.lastCallId ?? null);
        return label;
      };
      // 并行工具各自建立子 scope；其中一个内部推进自己的 lastCallId 不得影响另一个
      await Promise.all([
        runToolExecutionWithModelTrace({ toolName: "vision", toolCallId: "tc_a" }, async () => {
          await tick();
          noteAgentStreamCallStarted("mc_c2_vision");
          await tick();
          parents.push(currentModelTraceScope()?.lastCallId ?? null);
        }),
        runToolExecutionWithModelTrace({ toolName: "approval", toolCallId: "tc_b" }, async () => {
          await tick();
          await tick();
          await tick();
          // C2(vision) 的推进不得泄漏进 approval 分支
          parents.push(currentModelTraceScope()?.lastCallId ?? null);
        }),
        Promise.resolve(tool("plain")),
      ]);
    });
    // 两个工具分支的 parent 都是 C1，绝不能串成 approval→vision
    expect(parents).toEqual(["mc_c1", "mc_c2_vision", "mc_c1"]);
  });

  it("子 scope 继承 traceId 与 refs，附 toolName/toolCallId", () => {
    const child = runWithNewModelTrace({ origin: "user_turn", refs: { sessionId: "s1" } }, () => {
      noteAgentStreamCallStarted("mc_c1");
      return runToolExecutionWithModelTrace({ toolName: "media_generate-image", toolCallId: "tc_9" }, () =>
        currentModelTraceScope()!);
    });
    expect(child.traceId).toMatch(/^mt_/);
    expect(child.causalParentCallId).toBe("mc_c1");
    expect(child.refs).toMatchObject({
      sessionId: "s1",
      toolName: "media_generate-image",
      toolCallId: "tc_9",
    });
  });

  it("无外层 scope 时工具原样执行（不伪造 trace）", () => {
    const scope = runToolExecutionWithModelTrace({ toolName: "t" }, () =>
      currentModelTraceScope());
    expect(scope).toBeNull();
  });
});

describe("origin / refs 边界（§五十五/§六十八）", () => {
  it("origin 是有限枚举，非法值归 unknown", () => {
    const scope = runWithNewModelTrace({ origin: "made-up-origin" as any }, () =>
      currentModelTraceScope()!);
    expect(scope.origin).toBe("unknown");
    expect(MODEL_TRACE_ORIGINS).toContain("user_turn");
    expect(isModelTraceOrigin("diary")).toBe(true);
    expect(isModelTraceOrigin("diary-ish")).toBe(false);
  });

  it("refs 上限 8 键、值截断 128", () => {
    const many: Record<string, string> = {};
    for (let index = 0; index < 12; index += 1) many[`k${index}`] = "v";
    const scope = runWithNewModelTrace({
      origin: "user_turn",
      refs: { long: "x".repeat(300), ...many },
    }, () => currentModelTraceScope()!);
    expect(Object.keys(scope.refs!)).toHaveLength(8);
    expect(scope.refs!.long.length).toBeLessThanOrEqual(130);
    expect(scope.refs!.long).toContain("…");
  });
});

describe("runWithModelTrace（显式 scope 进入）", () => {
  it("显式 scope 可作为子上下文进入（冻结快照语义）", () => {
    const outer = runWithNewModelTrace({ origin: "user_turn" }, () => {
      const parentScope = currentModelTraceScope()!;
      const childScope = {
        traceId: parentScope.traceId,
        origin: "user_turn",
        causalParentCallId: "mc_c9",
        refs: null,
        lastCallId: "mc_c9",
      };
      return runWithModelTrace(childScope, () => currentModelTraceScope()!);
    });
    expect(outer.causalParentCallId).toBe("mc_c9");
    expect(outer.lastCallId).toBe("mc_c9");
  });
});
