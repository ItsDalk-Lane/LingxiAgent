/**
 * R00-T05 / R00-A10｜旧缺陷不变成标准：会话 JSONL 损坏尾记录的静默降级。
 *
 * 已登记缺陷（fixtures/sessions/corrupted-tail/old-deviation.json）：
 * 旧实现读取含损坏尾记录的会话时，宽容层静默丢弃该行；路由与生产入口返回成功、
 * 响应体零标注（用户无从得知消息被丢弃）。磁盘上会留下 .repair.json 回执（部分
 * 缓解），但该回执不进入任何用户可见响应；宽容层连回执都不产生。
 *
 * 违反的规则（expected.json rule_sources）：
 *  - AGENTS.md 红线「禁止静默降级（错误要么抛要么显式降级并标注）」；
 *  - 仓库既有先例：超长行修复会落盘 .repair.json 并返回 repaired/projected 计数
 *    （core/session-jsonl-file.ts），说明「显式降级并标注」在本仓库是可达且既定的做法。
 *
 * 本测试机器验证三件事：
 *  1. 严格层确实能识别该输入（抛 session_branch_invalid_json）——证明「响亮失败」是
 *     旧代码库内已存在的可选路径，不是对新系统的额外发明；
 *  2. 旧生产读取路径命中 forbidden_outcome.silent_success（成功返回且零标注）；
 *  3. 新期望（expected.json acceptable_outcomes）不允许任何静默成功形态——
 *     未来 Rust 实现按同一夹具回放时必须命中 acceptable_outcomes 之一。
 * 新期望不得为迁就旧 bug 改写：expected.json 的不变量独立于旧实现行为存在。
 */

import fs from "fs";
import os from "os";
import path from "path";
import { afterAll, describe, expect, it } from "vitest";

if (process.env.LINGXI_MIGRATION_BLOCK_NETWORK !== "0") {
  const { installExternalNetworkGuard } = await import("./network-guard.ts");
  installExternalNetworkGuard("tests/migration/r00-a10-old-defect.test.ts");
}

const { readCurrentSessionBranch, readSessionMessages } = await import("../../lib/session-jsonl.ts");
const { loadSessionHistoryMessages } = await import("../../core/message-utils.ts");

const FIXTURES_ROOT = path.resolve(import.meta.dirname, "fixtures");

function fixturePath(...segments: string[]): string {
  const target = path.resolve(FIXTURES_ROOT, ...segments);
  if (target === FIXTURES_ROOT || !target.startsWith(FIXTURES_ROOT + path.sep)) {
    throw new Error(`fixture path escapes fixtures root: ${segments.join("/")}`);
  }
  return target;
}

const tmpDirs: string[] = [];
function makeTmpDir(prefix: string): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  tmpDirs.push(dir);
  return dir;
}
afterAll(() => {
  for (const dir of tmpDirs) fs.rmSync(dir, { recursive: true, force: true });
});

describe("R00-A10｜损坏尾记录：旧实现静默降级被登记为偏差，不成为新标准", () => {
  const tmp = makeTmpDir("hana-mig-a10-");
  const sessionPath = path.join(tmp, "corrupted-tail.jsonl");
  fs.copyFileSync(fixturePath("sessions/corrupted-tail/session.jsonl"), sessionPath);

  const standard = JSON.parse(fs.readFileSync(fixturePath("sessions/corrupted-tail/expected.json"), "utf8"));
  const deviation = JSON.parse(fs.readFileSync(fixturePath("sessions/corrupted-tail/old-deviation.json"), "utf8"));

  it("严格层对同一输入抛结构化错误（响亮失败是旧代码库既有能力）", () => {
    let caught: any = null;
    try {
      readCurrentSessionBranch(sessionPath);
    } catch (error) {
      caught = error;
    }
    expect(caught).toBeTruthy();
    expect(caught.name).toBe("SessionBranchError");
    expect(caught.code).toBe("session_branch_invalid_json");
    expect(caught.details?.line).toBe(6);
  });

  it("旧宽容层静默丢弃坏行且零标注（命中 forbidden_outcome.silent_success）", () => {
    const lenient = readSessionMessages(sessionPath);
    const payload = JSON.stringify(lenient);
    // 合法链 u1,a1,a2 可读（toolResult 不入 display 消息）
    expect(lenient.messages.map((m: any) => m.role)).toEqual(["user", "assistant", "assistant"]);
    // 静默证据：无 dropped/corrupt/repair/degradation 任何标注
    expect(payload).not.toMatch(/dropp|corrupt|repair|degrad/i);
  });

  it("旧生产入口响应零标注（磁盘回执存在但不进响应——分层事实如实登记）", async () => {
    const tmp2 = makeTmpDir("hana-mig-a10-prod-");
    const prodPath = path.join(tmp2, "corrupted-tail.jsonl");
    fs.copyFileSync(fixturePath("sessions/corrupted-tail/session.jsonl"), prodPath);
    const production = await loadSessionHistoryMessages(null, prodPath);
    expect(production.length).toBeGreaterThan(0);
    // 响应级：无 dropped/corrupt/repair/degradation 任何标注
    expect(JSON.stringify(production.map((m: any) => Object.keys(m)))).not.toMatch(/dropp|corrupt|repair|degrad/i);
    // 磁盘级：.repair.json 回执存在（部分缓解，但用户不可见）
    expect(fs.existsSync(path.join(path.dirname(prodPath), path.basename(prodPath) + ".repair.json"))).toBe(true);
  });

  it("新期望（acceptable_outcomes）不接受旧实现的静默成功形态", () => {
    // 旧实现实际结果归类（以用户可见响应为准；磁盘回执不改变响应级静默事实）
    const oldOutcomeFitsStructuredError = false; // 生产路径未抛错
    const oldOutcomeFitsAnnotatedDegradation = false; // 响应无 droppedCorruptLines 标注
    const oldOutcomeIsSilentSuccess = !oldOutcomeFitsStructuredError && !oldOutcomeFitsAnnotatedDegradation;

    const forbidden = standard.forbidden_outcomes.some((f: any) => f.mode === "silent_success");
    expect(forbidden).toBe(true);
    expect(oldOutcomeIsSilentSuccess).toBe(true);

    // acceptable_outcomes 至少包含「结构化错误」与「带计数标注的显式降级」两种合规出路，
    // 且两种出路都能表达被丢弃的行数（错误 details.line / droppedCorruptLines）。
    const modes = standard.acceptable_outcomes.map((a: any) => a.mode).sort();
    expect(modes).toEqual(["annotated_degradation", "structured_error"]);
    const structured = standard.acceptable_outcomes.find((a: any) => a.mode === "structured_error");
    expect(structured.shape.code).toBe("session_branch_invalid_json");
    const annotated = standard.acceptable_outcomes.find((a: any) => a.mode === "annotated_degradation");
    expect(annotated.shape.required_annotation.droppedCorruptLines).toBe(1);
  });

  it("偏差登记与规则来源完整（偏差可复核，不是口头声明）", () => {
    expect(deviation.record_kind).toBe("OLD_SYSTEM_DEVIATION_NOT_STANDARD");
    expect(deviation.violated_rule).toContain("NO-SILENT-DEGRADATION");
    expect(deviation.deviation_sites.length).toBeGreaterThanOrEqual(3);
    expect(deviation.test_evidence.length).toBeGreaterThanOrEqual(2);
    expect(deviation.partial_mitigations.length).toBeGreaterThanOrEqual(1);
    // 登记的 old_actual 与标准一起构成机器可复核偏差：路由 200 + 响应零标注
    expect(deviation.old_actual.route.status).toBe(200);
    expect(deviation.old_actual.route.bodyAnnotationAbsent).toBe(true);
    expect(deviation.old_actual.route.messageCount).toBe(3);
    expect(deviation.old_actual.lenientReader.annotationAbsent).toBe(true);
    // 新期望的规则来源引用了仓库红线与既有显式修复先例
    const ruleIds = standard.rule_sources.map((r: any) => r.id);
    expect(ruleIds).toContain("AGENTS-REDLINE-NO-SILENT-DEGRADATION");
    expect(ruleIds).toContain("REPAIR-RECEIPT-PRECEDENT");
  });
});
