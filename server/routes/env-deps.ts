/**
 * env-deps — 环境依赖检测的 HTTP 面（/api/system/env-deps/*）
 *
 * GET  /api/system/env-deps           读缓存报告（?refresh=1 强制重探）
 * POST /api/system/env-deps/refresh   强制重探并返回新报告
 *
 * 项目感知需要工作区根：?agentId= 指定助手的工作区，缺省取第一个助手。
 * 探测本身永不抛错，单依赖失败收敛为 missing；整体失败才 500。
 */

import { Hono } from "hono";
import { detectEnvDeps, type EnvDepsReport } from "../../lib/env-deps/detect.ts";

function resolveWorkspaceRoots(engine: any, agentId: string | null): string[] {
  const roots = new Set<string>();
  try {
    const id = agentId || engine.listAgents?.()?.[0]?.id || null;
    if (id) {
      const explicit = typeof engine.getExplicitHomeCwd === "function" ? engine.getExplicitHomeCwd(id) : null;
      const home = typeof engine.getHomeCwd === "function" ? engine.getHomeCwd(id) : null;
      if (explicit) roots.add(String(explicit));
      if (home) roots.add(String(home));
    }
  } catch { /* 根不可达时降级为无项目感知 */ }
  return [...roots];
}

export async function buildEnvDepsReport(engine: any, agentId: string | null, force: boolean): Promise<EnvDepsReport> {
  const workspaceRoots = resolveWorkspaceRoots(engine, agentId);
  return detectEnvDeps({ workspaceRoots, force });
}

export function createEnvDepsRoute(engine: any) {
  const route = new Hono();

  route.get("/system/env-deps", async (c) => {
    try {
      const agentId = c.req.query("agentId") || null;
      const force = c.req.query("refresh") === "1";
      const report = await buildEnvDepsReport(engine, agentId, force);
      return c.json(report);
    } catch (err: any) {
      return c.json({ error: "env_deps_detection_failed", message: String(err?.message || err) }, 500);
    }
  });

  route.post("/system/env-deps/refresh", async (c) => {
    try {
      const agentId = c.req.query("agentId") || null;
      const report = await buildEnvDepsReport(engine, agentId, true);
      return c.json(report);
    } catch (err: any) {
      return c.json({ error: "env_deps_detection_failed", message: String(err?.message || err) }, 500);
    }
  });

  return route;
}
