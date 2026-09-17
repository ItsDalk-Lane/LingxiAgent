/**
 * learn-lesson-tool.ts — learn_lesson 工具
 *
 * 把一条可复用教训结晶成用户技能池里的 SKILL.md（单文件技能），
 * 并往经验库写一条索引。与 install_skill 的差别：install_skill 安装外部
 * 完整技能包，本工具把 agent 自己踩坑得到的教训直接写成新技能。
 *
 * 安全策略（与 install_skill 同一套）：
 *   - 大小上限共用 MAX_SKILL_SIZE；
 *   - 名称过 sanitizeSkillName（白名单字符，路径逃逸无从构造）；
 *   - 内容过 guard 安全审查；未通过走 risk_accepted + risk_confirmation_token
 *     确认链（token 绑定内容摘要，10 分钟有效）；
 *   - 同名技能已存在 → 拒绝，不覆盖；
 *   - 写入后显式触发技能 reload（onLearned 回调），watcher 的 1s 热更新兜底。
 */

import fs from "node:fs";
import path from "node:path";
import { Type } from "../pi-sdk/index.ts";
import { t } from "../i18n.ts";
import {
  MAX_SKILL_SIZE,
  safetyReview,
  createRiskConfirmationToken,
  consumeRiskAcceptance,
} from "./install-skill.ts";
import {
  installSkillPackageFromContent,
  sanitizeSkillName,
} from "../skills/skill-package-installer.ts";
import { recordEntry } from "./experience.ts";

/** 经验库里沉淀索引的固定分类（autolearn 共用同一分类） */
export const LESSON_EXPERIENCE_CATEGORY = "learned lessons";

/** 组装单文件技能的 SKILL.md 正文（frontmatter name/description + 教训正文）。 */
export function assembleLessonSkillContent(safeName: string, description: string, lesson: string): string {
  return [
    "---",
    `name: ${safeName}`,
    `description: ${JSON.stringify(description)}`,
    "---",
    "",
    lesson,
    "",
  ].join("\n");
}

/**
 * installLearnedLesson — learn_lesson 工具与 autolearn 服务共用的落盘核心。
 * 调用方各自负责前置闸门（工具：总开关+审查+风险确认链；autolearn：建议卡
 * 用户确认）；这里只做：写入技能池（installer 内置 symlink/落点校验）→
 * 经验库索引（失败不吞落盘事实，experienceIndexed 如实标注）。
 */
export async function installLearnedLesson({ agentDir, userSkillsDir, safeName, content, description, lesson }: any) {
  const installed = await installSkillPackageFromContent({
    content,
    skillName: safeName,
    installDir: userSkillsDir,
    owner: "user",
    defaultEnabled: false,
  });

  const experienceDir = path.join(agentDir, "experience");
  const indexPath = path.join(agentDir, "experience.md");
  const summary = lesson.split("\n").map((l: string) => l.trim()).filter(Boolean)[0] || description;
  let experienceIndexed = false;
  try {
    experienceIndexed = recordEntry(
      experienceDir,
      indexPath,
      LESSON_EXPERIENCE_CATEGORY,
      `${safeName}: ${summary.length > 80 ? summary.slice(0, 77) + "…" : summary}`,
    ).added === true;
  } catch { /* 索引失败不吞掉技能本体已落盘的事实——调用方 details 里如实标注 */ }

  return { skillName: installed.name, skillFilePath: installed.filePath, experienceIndexed };
}

/**
 * @param {object} opts
 * @param {string} opts.agentDir             agent 数据目录（经验库落点）
 * @param {() => string|null} opts.getUserSkillsDir  用户技能池目录（延迟求值）
 * @param {() => boolean} opts.isEnabled     learn_skills 总开关
 * @param {() => object} opts.resolveGuardModel  resolveAuxiliaryModelFresh("guard") 结果
 * @param {(skillName: string) => Promise<void>} opts.onLearned  写入完成后的回调（reload + 当前 agent 启用）
 */
export function createLearnLessonTool({ agentDir, getUserSkillsDir, isEnabled, resolveGuardModel, onLearned }: any) {
  // 与 install_skill 各自持有 token 表：sourceKey 不同，互不消费
  const pendingRiskConfirmations = new Map();

  return {
    name: "learn_lesson",
    label: "Learn Lesson",
    description:
      "Crystallize a reusable lesson into a new skill (SKILL.md) in the shared skill pool, enabled for the current Agent, and index it in the experience library. Use when you hit a pitfall or discover a reusable technique worth keeping across sessions. Content passes the same safety review as install_skill; if the result carries requiresRiskConfirmation, explain the risk to the user and only retry with risk_accepted=true plus the returned risk_confirmation_token after explicit user confirmation. A same-name skill is rejected, never overwritten — pick a distinct name.",
    sessionPermission: {
      resolveInvocation: (params: any = {}) => {
        if (typeof params.name !== "string" || typeof params.lesson !== "string") return null;
        return {
          action: "learn",
          kind: "review",
          capability: "learn_lesson.learn",
          sideEffect: { kind: "shared_executable_content_install" },
        };
      },
    },
    parameters: Type.Object({
      name: Type.String({ description: "Skill name: 1-64 chars of letters, digits, '_' or '-', starting with a letter/digit. Used as the skill directory name." }),
      description: Type.String({ description: "One line: when this lesson applies. Becomes the skill description used for discovery, so write it as a trigger condition." }),
      lesson: Type.String({ description: "The lesson body in markdown: what went wrong / what works, and the concrete rule to follow next time." }),
      risk_accepted: Type.Optional(
        Type.Boolean({ description: "Set true only after the user explicitly confirms learning despite a failed safety review warning." })
      ),
      risk_confirmation_token: Type.Optional(
        Type.String({ description: "Opaque token returned by a previous requiresRiskConfirmation result. Required with risk_accepted=true." })
      ),
    }),
    execute: async (_toolCallId, params, _signal, _onUpdate, _ctx) => {
      // ── 总开关（learn_skills.enabled，默认开）──
      let enabled = false;
      try { enabled = typeof isEnabled === "function" && isEnabled() === true; } catch { enabled = false; }
      if (!enabled) {
        return {
          content: [{ type: "text", text: t("error.installSkillDisabled") }],
          details: {},
        };
      }

      const name = typeof params.name === "string" ? params.name.trim() : "";
      const description = typeof params.description === "string" ? params.description.trim() : "";
      const lesson = typeof params.lesson === "string" ? params.lesson.trim() : "";
      if (!name || !description || !lesson) {
        return {
          content: [{ type: "text", text: t("error.learnLessonEmptyInput") }],
          details: {},
        };
      }

      const safeName = sanitizeSkillName(name);
      if (!safeName) {
        return {
          content: [{ type: "text", text: t("error.installSkillNameInvalid", { name: `: ${name}` }) }],
          details: {},
        };
      }

      const userSkillsDir = typeof getUserSkillsDir === "function" ? getUserSkillsDir() : null;
      if (!userSkillsDir) {
        return {
          content: [{ type: "text", text: t("error.learnLessonPoolUnavailable") }],
          details: {},
        };
      }

      // ── 同名冲突拒绝（先于安全审查，不白烧一次 guard 调用）──
      if (fs.existsSync(path.join(userSkillsDir, safeName))) {
        return {
          content: [{ type: "text", text: t("error.learnLessonConflict", { name: safeName }) }],
          details: { conflict: true, skillName: safeName },
        };
      }

      // ── 组装 SKILL.md：frontmatter（name/description）+ 教训正文 ──
      const content = assembleLessonSkillContent(safeName, description, lesson);
      if (content.length > MAX_SKILL_SIZE) {
        return {
          content: [{ type: "text", text: t("error.installSkillSizeLimit", { size: Math.round(content.length / 1000), max: MAX_SKILL_SIZE / 1000 }) }],
          details: {},
        };
      }

      // ── 安全审查（guard 槽，fail-closed + 风险确认链）──
      const sourceKey = `learn_lesson:${safeName}`;
      const review = await safetyReview(content, resolveGuardModel);
      let safetyPassed = false;
      let riskOverride = false;
      let riskReason = "";
      if (!review.safe) {
        const acceptance = consumeRiskAcceptance(pendingRiskConfirmations, params, {
          sourceKey,
          skillContent: content,
        });
        if (!acceptance.accepted) {
          const token = createRiskConfirmationToken(pendingRiskConfirmations, {
            sourceKey,
            skillContent: content,
            reason: review.reason,
          });
          return {
            content: [{ type: "text", text: t("error.learnLessonSafetyFailed", { reason: review.reason }) }],
            details: {
              safetyReview: false,
              requiresRiskConfirmation: true,
              riskConfirmationToken: token,
              riskReason: review.reason,
              riskAccepted: false,
              ...(acceptance.rejection ? { riskAcceptanceRejection: acceptance.rejection } : {}),
              nextAction: "ask_user_then_retry_with_risk_accepted",
            },
          };
        }
        riskOverride = true;
        riskReason = review.reason || "";
      } else {
        safetyPassed = true;
      }

      // ── 写入技能池 + 经验索引（与 autolearn 共用的落盘核心）──
      let installed;
      try {
        installed = await installLearnedLesson({ agentDir, userSkillsDir, safeName, content, description, lesson });
      } catch (err: any) {
        return {
          content: [{ type: "text", text: err?.code === "SKILL_INVALID_NAME"
            ? t("error.installSkillNameInvalid", { name: `: ${safeName}` })
            : (err?.message || String(err)) }],
          details: {},
        };
      }
      const experienceIndexed = installed.experienceIndexed;

      // ── 显式 reload + 当前 agent 启用（watcher 1s 热更新兜底）──
      await onLearned?.(installed.skillName);

      const safetyNote = safetyPassed
        ? t("error.installSkillSafetyPassed")
        : (riskOverride ? t("error.installSkillSafetyOverride", { reason: riskReason }) : "");
      return {
        content: [{ type: "text", text: t("error.learnLessonSuccess", { name: installed.skillName }) + (safetyNote ? "\n" + safetyNote : "") }],
        details: {
          skillName: installed.skillName,
          skillFilePath: installed.skillFilePath,
          safetyReview: safetyPassed,
          riskOverride,
          ...(riskReason ? { riskReason } : {}),
          experienceIndexed,
        },
      };
    },
  };
}
