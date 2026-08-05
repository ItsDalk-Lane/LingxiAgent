import { describe, expect, it, vi } from "vitest";
import {
  DreaminaCapabilityError,
  createDreaminaCapabilityDiscovery,
  findDreaminaMode,
  findDreaminaModel,
  parseDreaminaCapabilityOutputs,
  parseDreaminaVersion,
  resolveDreaminaExecutable,
} from "../plugins/jimeng-cli/lib/dreamina-capabilities.ts";

const TEXT_TO_IMAGE_HELP = `
Usage:
  dreamina text2image [flags]

Supported combinations:
- model_version: 3.0, 3.1, 4.0, 4.1, 4.5, 4.6, 4.7, 5.0
- ratio: 21:9, 16:9, 3:2, 4:3, 1:1, 3:4, 2:3, 9:16
- 3.0/3.1 -> resolution_type 1k or 2k
- 4.0/4.1/4.5/4.6/4.7/5.0 -> resolution_type 2k or 4k
`;

const IMAGE_TO_IMAGE_HELP = `
Usage:
  dreamina image2image [flags]

Upload 1 to 10 local images, then submit a Dreamina image-to-image task.

Supported combinations:
- model_version: 4.0, 4.1, 4.5, 4.6, 4.7, 5.0
- ratio: 21:9, 16:9, 3:2, 4:3, 1:1, 3:4, 2:3, 9:16
- resolution_type: 2k, 4k
`;

const TEXT_TO_VIDEO_HELP = `
Usage:
  dreamina text2video [flags]

Supported combinations:
- model_version: seedance2.0, seedance2.0fast, seedance2.0_vip, seedance2.0fast_vip, seedance2.0mini
- ratio: 1:1, 3:4, 16:9, 4:3, 9:16, 21:9
- seedance2.0_vip -> video_resolution 720p, 1080p, or 4k; duration 4-15s
- all other models -> video_resolution 720p; duration 4-15s

Notes:
- default model_version: seedance2.0fast

Flags:
      --duration int   video duration in seconds; supported range: 4-15 (default 5)
`;

const IMAGE_TO_VIDEO_HELP = `
Usage:
  dreamina image2video [flags]

Supported combinations:
- advanced model_version values: seedance1.0fast, seedance1.0, seedance1.5pro, seedance2.0, seedance2.0fast, seedance2.0_vip, seedance2.0fast_vip, seedance2.0mini
- seedance2.0_vip -> video_resolution 720p, 1080p, or 4k
- all other models -> video_resolution 720p

Flags:
      --duration int   advanced controls only; supported duration ranges by model: seedance1.0fast/seedance1.0 -> 3-10, seedance1.5pro -> 4-12, seedance2.0 family/seedance2.0mini -> 4-15 (default 5)
`;

const HELP_OUTPUTS = {
  text2image: TEXT_TO_IMAGE_HELP,
  image2image: IMAGE_TO_IMAGE_HELP,
  text2video: TEXT_TO_VIDEO_HELP,
  image2video: IMAGE_TO_VIDEO_HELP,
};

function parseSnapshotMedia() {
  return parseDreaminaCapabilityOutputs(HELP_OUTPUTS);
}

describe("Dreamina CLI capability parsing", () => {
  it("builds image provider models from the CLI model and mode intersections", () => {
    const media = parseSnapshotMedia();
    expect(media.imageGeneration.defaultModelId).toBe("jimeng-image-5.0");
    expect(media.imageGeneration.models.map((model) => model.id)).toEqual([
      "jimeng-image-3.0",
      "jimeng-image-3.1",
      "jimeng-image-4.0",
      "jimeng-image-4.1",
      "jimeng-image-4.5",
      "jimeng-image-4.6",
      "jimeng-image-4.7",
      "jimeng-image-5.0",
    ]);

    const image30 = media.imageGeneration.models.find((model) => model.id === "jimeng-image-3.0");
    expect(image30).toMatchObject({
      protocolId: "jimeng-cli-images",
      inputs: ["text"],
      supportsEdit: false,
      resolutions: ["1k", "2k"],
    });
    expect((image30?.modes as Array<{ id: string }>).map((mode) => mode.id)).toEqual(["text2image"]);

    const image50 = media.imageGeneration.models.find((model) => model.id === "jimeng-image-5.0");
    expect(image50).toMatchObject({
      inputs: ["text", "image"],
      supportsEdit: true,
      resolutions: ["2k", "4k"],
    });
    const editMode = findDreaminaMode(image50, "image2image");
    expect(editMode).toMatchObject({
      defaults: { ratio: "3:2", resolution: "4k" },
      inputLimits: { referenceImages: { min: 1, max: 10 } },
      parameterSchema: {
        properties: {
          ratio: { enum: ["21:9", "16:9", "3:2", "4:3", "1:1", "3:4", "2:3", "9:16"] },
          resolution: { enum: ["2k", "4k"] },
        },
      },
    });
  });

  it("keeps video modes and per-model constraints aligned with each CLI command", () => {
    const media = parseSnapshotMedia();
    expect(media.videoGeneration.defaultModelId).toBe("seedance2.0fast");
    expect(media.videoGeneration.models.map((model) => model.id)).toEqual([
      "seedance2.0",
      "seedance2.0fast",
      "seedance2.0_vip",
      "seedance2.0fast_vip",
      "seedance2.0mini",
      "seedance1.0fast",
      "seedance1.0",
      "seedance1.5pro",
    ]);

    const legacy = media.videoGeneration.models.find((model) => model.id === "seedance1.0fast");
    expect(legacy).toMatchObject({
      inputs: ["image"],
      modes: [
        expect.objectContaining({
          id: "image2video",
          parameterSchema: expect.objectContaining({
            properties: expect.objectContaining({
              duration: expect.objectContaining({ minimum: 3, maximum: 10, default: 5 }),
            }),
          }),
        }),
      ],
      duration: { min: 3, max: 10 },
      resolutions: ["720p"],
    });

    const mini = media.videoGeneration.models.find((model) => model.id === "seedance2.0mini");
    expect((mini?.modes as Array<{ id: string }>).map((mode) => mode.id)).toEqual(["text2video", "image2video"]);
    expect(mini).toMatchObject({ duration: { min: 4, max: 15 }, resolutions: ["720p"] });

    const vip = media.videoGeneration.models.find((model) => model.id === "seedance2.0_vip");
    expect(vip).toMatchObject({ resolutions: ["720p", "1080p", "4k"] });
    expect(findDreaminaMode(vip, "text2video")).toMatchObject({
      defaults: { ratio: "16:9", duration: 5, video_resolution: "720p" },
      inputLimits: { referenceImages: { min: 0, max: 0 } },
      parameterSchema: {
        properties: {
          ratio: { enum: ["1:1", "3:4", "16:9", "4:3", "9:16", "21:9"] },
          duration: { minimum: 4, maximum: 15 },
          video_resolution: { enum: ["720p", "1080p", "4k"] },
        },
      },
    });
  });

  it("fails explicitly when capability output is empty or no longer parseable", () => {
    expect(() => parseDreaminaCapabilityOutputs({ ...HELP_OUTPUTS, text2video: "" }))
      .toThrowError(expect.objectContaining({ code: "output_unparseable" }));
    expect(() => parseDreaminaCapabilityOutputs({
      ...HELP_OUTPUTS,
      text2image: TEXT_TO_IMAGE_HELP.replace(/^- model_version:.+$/m, "- models changed format"),
    })).toThrowError(/model_version values/);
  });

  it("parses the structured CLI version and rejects opaque version text", () => {
    expect(parseDreaminaVersion(JSON.stringify({
      version: "2a20fff-dirty",
      commit: "2a20fff",
      build_time: "2026-06-26T06:36:39Z",
    }))).toEqual({
      version: "2a20fff-dirty",
      commit: "2a20fff",
      buildTime: "2026-06-26T06:36:39Z",
    });
    expect(() => parseDreaminaVersion("dreamina version unknown")).toThrow(DreaminaCapabilityError);
  });
});

describe("Dreamina CLI capability discovery", () => {
  it("uses execFile-style commands and skips help subprocesses while the fingerprint is unchanged", async () => {
    let mtimeMs = 100;
    const runCommand = vi.fn(async (_command: string, args: string[], _options: Record<string, unknown>) => {
      if (args[0] === "--version") {
        return { stdout: JSON.stringify({ version: "2a20fff-dirty", commit: "2a20fff" }) };
      }
      return { stdout: HELP_OUTPUTS[args[0] as keyof typeof HELP_OUTPUTS] };
    });
    const discovery = createDreaminaCapabilityDiscovery({
      resolveCommand: () => "/opt/dreamina-link",
      realpath: async () => "/opt/dreamina",
      statFile: async () => ({ mtimeMs, size: 2048, isFile: () => true }),
      runCommand,
      now: () => new Date("2026-07-15T10:00:00.000Z"),
    });

    const first = await discovery.refresh({ capability: "image_generation" });
    const second = await discovery.refresh({ capability: "video_generation" });
    expect(second).toBe(first);
    expect(first).toMatchObject({
      providerId: "jimeng-cli",
      version: { version: "2a20fff-dirty", commit: "2a20fff" },
      fingerprint: { path: "/opt/dreamina", version: "2a20fff-dirty", mtimeMs: 100, size: 2048 },
      discoveredAt: "2026-07-15T10:00:00.000Z",
    });
    expect(findDreaminaModel(first, "videoGeneration", "seedance2.0mini")).not.toBeNull();
    expect(runCommand).toHaveBeenCalledTimes(5);
    expect(runCommand.mock.calls.filter((call) => call[1][1] === "--help")).toHaveLength(4);
    for (const call of runCommand.mock.calls) {
      expect(call[2]).toMatchObject({ shell: false });
    }

    mtimeMs = 101;
    const third = await discovery.refresh();
    expect(third).not.toBe(first);
    expect(runCommand).toHaveBeenCalledTimes(10);
    expect(runCommand.mock.calls.filter((call) => call[1][1] === "--help")).toHaveLength(8);
  });

  it("reports missing executables and command failures with stable error codes", async () => {
    const missing = createDreaminaCapabilityDiscovery({ resolveCommand: () => null });
    await expect(missing.refresh()).rejects.toMatchObject({ code: "cli_missing" });

    const broken = createDreaminaCapabilityDiscovery({
      resolveCommand: () => "/opt/dreamina",
      realpath: async (value) => value,
      statFile: async () => ({ mtimeMs: 1, size: 1, isFile: () => true }),
      runCommand: async () => {
        throw new Error("exit 1");
      },
    });
    await expect(broken.refresh()).rejects.toMatchObject({ code: "command_failed" });
  });

  it("resolves explicit install paths and the per-user CLI location without a shell", () => {
    expect(resolveDreaminaExecutable({
      env: { DREAMINA_CLI_PATH: "/Applications/dreamina" },
      platform: "darwin",
      exists: (filePath) => filePath === "/Applications/dreamina/dreamina",
    })).toBe("/Applications/dreamina/dreamina");

    expect(resolveDreaminaExecutable({
      env: { PATH: "/usr/bin:/bin" },
      homeDir: "/Users/hana",
      platform: "darwin",
      which: () => null,
      exists: (filePath) => filePath === "/Users/hana/.local/bin/dreamina",
    })).toBe("/Users/hana/.local/bin/dreamina");
  });
});
