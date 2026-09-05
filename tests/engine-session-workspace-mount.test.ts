/**
 * engine.getSessionWorkspaceMount 委托回归（v0.1.33 左栏列表空白缺陷链）。
 *
 * 缺陷链（2026-09-05 定位）：switch 路由经 sessionWorkspaceMountFields(engine,
 * sessionPath) 只信 engine.getSessionWorkspaceMount（无 fallback，与 create 路由
 * 不同）回传会话工作台身份；真实 engine 此前只暴露了 getSessionWorkspaceFolders /
 * getSessionAuthorizedFolders 两个委托，漏了 getSessionWorkspaceMount——路由的
 * optional-call 静默拿到 undefined，switch 回包永远不带 workspaceMountId/
 * workspaceLabel，客户端 desk 落成本地目录键，与列表投影里带 mountId 的会话
 * 严格互斥（session-sections.ts 本地作用域排除 mount 会话）→ 左栏列表空白。
 *
 * 本测试锁 engine 这环；路由发射（engine 提供方法时回包带 mount）由
 * tests/sessions-route.test.ts「switch keeps viewer visible…」锁定，客户端
 * desk 恢复由 desktop stores 的 session-new-session-* 测试锁定。
 */
import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LingxiEngine } from "../core/engine.ts";

describe("LingxiEngine.getSessionWorkspaceMount delegation", () => {
  let tmpDir: string | null = null;
  let engine: LingxiEngine | null = null;

  afterEach(async () => {
    if (engine) await engine.dispose();
    if (tmpDir) fs.rmSync(tmpDir, { recursive: true, force: true });
    tmpDir = null;
    engine = null;
  });

  function createEngine() {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-engine-ws-mount-"));
    engine = new LingxiEngine({
      lingxiHome: tmpDir,
      productDir: tmpDir,
      agentId: "lingxi",
    } as never);
    return engine;
  }

  it("exposes getSessionWorkspaceMount and delegates to the session coordinator with the requested path", () => {
    const e = createEngine();

    // 修复前：engine 没有这个方法（路由 optional-call 静默拿 undefined）。
    expect(typeof e.getSessionWorkspaceMount).toBe("function");

    const expected = { mountId: "mount_docs", label: "Docs" };
    const spy = vi.spyOn(e._sessionCoord, "getSessionWorkspaceMount").mockReturnValue(expected as never);

    const result = e.getSessionWorkspaceMount("/tmp/agents/lingxi/sessions/probe.jsonl");

    expect(spy).toHaveBeenCalledWith("/tmp/agents/lingxi/sessions/probe.jsonl");
    expect(result).toEqual(expected);
  });

  it("defaults the delegation target to the engine's current session path", () => {
    const e = createEngine();
    const spy = vi.spyOn(e._sessionCoord, "getSessionWorkspaceMount").mockReturnValue(null);

    const result = e.getSessionWorkspaceMount();

    // 冷引擎 currentSessionPath 为 null,委托原样传 null(coordinator 对 falsy 返回 null)。
    expect(spy).toHaveBeenCalledWith(null);
    expect(result).toBeNull();
  });
});
