import { t } from "../../../lib/i18n.ts";

export const name = "generate-speech";
export const description = t("toolDef.generateSpeech.description");

export const sessionPermission = {
  kind: "external_side_effect",
  describeSideEffect: (input: any = {}) => ({
    kind: "external_generation",
    summary: `Generate speech audio${input.provider ? ` via provider ${input.provider}` : ""}.`,
    ruleId: "media-speech-generation",
  }),
};

export const parameters = {
  type: "object",
  properties: {
    prompt: { type: "string", description: t("toolDef.generateSpeech.promptDesc") },
    voice: { type: "string", description: t("toolDef.generateSpeech.voiceDesc") },
    speed: { type: "number", description: t("toolDef.generateSpeech.speedDesc") },
    format: { type: "string", enum: ["mp3", "opus", "aac", "flac", "wav"], description: t("toolDef.generateSpeech.formatDesc") },
    model: { type: "string", description: t("toolDef.generateSpeech.modelDesc") },
    provider: { type: "string", description: t("toolDef.generateSpeech.providerDesc") },
  },
  required: ["prompt"],
};

function present(value) {
  return value !== undefined && value !== null && value !== "";
}

function mediaInput(input: any = {}) {
  return {
    prompt: input.prompt,
    ...(present(input.voice) ? { voice: input.voice } : {}),
    ...(present(input.speed) ? { speed: input.speed } : {}),
    ...(present(input.format) ? { format: input.format } : {}),
    ...(present(input.model) ? { model: input.model } : {}),
    ...(present(input.provider) ? { provider: input.provider } : {}),
  };
}

function sessionPayload(ctx: any = {}, input) {
  return {
    ...(ctx.sessionId ? { sessionId: ctx.sessionId } : {}),
    ...(ctx.sessionPath ? { sessionPath: ctx.sessionPath } : {}),
    ...(ctx.sessionRef ? { sessionRef: ctx.sessionRef } : {}),
    input,
    ...(ctx.bridgeContext ? { bridgeContext: ctx.bridgeContext } : {}),
    pluginId: ctx.pluginId || "media",
  };
}

export async function execute(input: any = {}, ctx: any = {}) {
  if (typeof ctx?.bus?.request !== "function") {
    return { content: [{ type: "text", text: t("toolDef.generateSpeech.notInitialized") }] };
  }

  let result;
  try {
    result = await ctx.bus.request("media:generate-speech", sessionPayload(ctx, mediaInput(input)));
  } catch (err) {
    return {
      content: [{ type: "text", text: t("toolDef.generateSpeech.submitFailed", { error: err?.message || t("plugin.imageGen.unknownError") }) }],
    };
  }

  const tasks = Array.isArray(result?.tasks)
    ? result.tasks.filter((task) => task && typeof task.taskId === "string" && task.taskId)
    : [];
  if (!result?.ok || tasks.length === 0) {
    return {
      content: [{ type: "text", text: t("toolDef.generateSpeech.submitFailedUnknown") }],
    };
  }

  return {
    content: [{ type: "text", text: t("toolDef.generateSpeech.submitted") }],
    details: {
      mediaGeneration: {
        kind: "speech",
        batchId: result.batchId,
        prompt: result.prompt || input.prompt,
        delivery: result.delivery,
        tasks,
      },
    },
  };
}
