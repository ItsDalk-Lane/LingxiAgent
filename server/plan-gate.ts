/**
 * plan-gate.ts — 计划模式收工硬闸
 *
 * 挂在与 loop-bus-handlers 同一条 turn 事件缝上（纯观察，不进轮内）。
 * 计划模式（只读权限档）下，一轮以「没写计划文件、没经 ask_user 交决策、
 * 没退出计划模式」收场时，注入一条提醒并续跑；每会话连推上限
 * MAX_CONSECUTIVE_NUDGES 防死循环，闸门一旦满足（写了计划/问过用户/
 * 退出计划模式/用户中止）即复位。
 *
 * 诚实边界：闸门只看结构信号（写文件事件、提问卡事件、权限档），
 * 不评判计划内容好坏；连推上限用尽就停手，把控制权交还用户。
 */
import fs from "fs";
import { isPlanFileWrite, planFilePathForSession } from "../lib/plan-mode/plan-file.ts";
import { isReadOnlyPermissionMode } from "../core/session-permission-mode.ts";

export const PLAN_GATE_MESSAGE_TYPE = "plan-gate-nudge";
export const MAX_CONSECUTIVE_NUDGES = 3;

function buildNudgeMessage(sessionPath, planExists, nudge, maxNudges) {
  const planPath = planFilePathForSession(sessionPath);
  const content = [
    `<hana-plan-gate nudge="${nudge}/${maxNudges}">`,
    `Plan mode (read-only) is still on, and the last turn ended without delivering a decision point. In plan mode a turn must end with one of:`,
    `1. Write or update the plan file — the ONLY writable file in plan mode: ${planPath}`,
    `2. Ask the user a decision question with the ask_user tool (batch related questions into one call, mark your recommended option).`,
    planExists
      ? `The plan file already exists; update it if the direction changed, or ask via ask_user if you are waiting on the user.`
      : `No plan file exists yet; draft the plan there now.`,
    `Do not end another plan-mode turn with plain analysis only.`,
    `</hana-plan-gate>`,
  ].join("\n");
  return {
    customType: PLAN_GATE_MESSAGE_TYPE,
    content,
    display: false,
    details: { schemaVersion: 1, kind: "plan_gate_nudge", nudge, maxNudges },
  };
}

/**
 * @param {object} bus   hub.eventBus（subscribe(event, sessionPath)）
 * @param {object} deps
 * @param {(sessionPath: string) => string} deps.getPermissionMode
 * @param {(sessionPath: string, message: object) => Promise<any>} deps.deliver
 * @param {object} [deps.log]
 * @param {(path: string) => boolean} [deps.fileExists]  测试可注入；默认 fs.existsSync
 */
export function registerPlanGateHandler(bus, deps) {
  const log = deps?.log || console;
  const fileExists = deps?.fileExists || ((p: string) => {
    try { return fs.existsSync(p); } catch { return false; }
  });
  /** sessionPath → { planWritten, askedUser, nudges } */
  const turns = new Map();

  const onTurnEnd = async (sessionPath) => {
    const flags = turns.get(sessionPath);
    turns.delete(sessionPath);
    const mode = deps.getPermissionMode?.(sessionPath);
    if (!isReadOnlyPermissionMode(mode)) return;          // 已退出计划模式：闸门休眠
    if (flags?.planWritten || flags?.askedUser) return;   // 已交决策：闸门满足
    const nudges = (flags?.nudges ?? 0) + 1;
    if (nudges > MAX_CONSECUTIVE_NUDGES) {
      // 连推上限用尽：按住计数不归零（归零会让下一轮重新开推），
      // 直到闸门满足/退出计划模式/用户中止把它清掉。
      turns.set(sessionPath, { planWritten: false, askedUser: false, nudges: MAX_CONSECUTIVE_NUDGES });
      return;
    }
    // 先同步记账再投递：turn_start/turn_end 连发（同 tick 爆发）时计数不丢；
    // 投递失败也算推过一次（防失败重推风暴），只记日志。
    turns.set(sessionPath, { planWritten: false, askedUser: false, nudges });
    const planPath = planFilePathForSession(sessionPath);
    const planExists = !!(planPath && fileExists(planPath));
    try {
      await deps.deliver(sessionPath, buildNudgeMessage(sessionPath, planExists, nudges, MAX_CONSECUTIVE_NUDGES));
    } catch (err) {
      // 会话可能已销毁/换代——观察钩子绝不把失败抛回总线
      log.warn?.(`[plan-gate] nudge delivery failed: ${err?.message}`);
    }
  };

  return bus.subscribe((event, sessionPath) => {
    if (!sessionPath || !event?.type) return;
    try {
      if (event.type === "turn_start") {
        const prev = turns.get(sessionPath);
        turns.set(sessionPath, { planWritten: false, askedUser: false, nudges: prev?.nudges ?? 0 });
      } else if (event.type === "tool_execution_end" && event.isError !== true) {
        const flags = turns.get(sessionPath);
        if (flags && isPlanFileWrite(event.toolName, event.args, sessionPath)) {
          flags.planWritten = true;
        }
      } else if (event.type === "session_confirmation") {
        if (event.request?.kind === "ask_user") {
          const flags = turns.get(sessionPath);
          if (flags) flags.askedUser = true;
        }
      } else if (event.type === "turn_end") {
        if (event.aborted === true) {
          // 用户中止：复位，不追推
          turns.delete(sessionPath);
          return;
        }
        void onTurnEnd(sessionPath);
      }
    } catch {
      // 观察钩子不允许影响主流程
    }
  });
}
