import { createHash } from "crypto";
import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  assembleTrainManifest,
  assertArtifactsMatchForResume,
  computeMirrors,
  computeNextTrain,
  computeTrainReleaseTag,
  diffManifestArtifacts,
  discoverBoxes,
  parseArgs,
  parseRendererArchiveName,
  parseServerArchiveName,
  publishChannel,
  releaseExistsFromExec,
  releaseInfoFromExec,
  SHELL_COMPAT_FLOOR,
} from "../scripts/publish-train.mjs";

const tempDirs: string[] = [];

function makeTempDir(prefix: string) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  tempDirs.push(dir);
  return dir;
}

function writeSignedManifestFixture(destDir: string, filename: string, manifest: object) {
  fs.writeFileSync(path.join(destDir, filename), JSON.stringify(manifest));
  fs.writeFileSync(path.join(destDir, `${filename}.sig`), "test-signature");
}

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
  vi.restoreAllMocks();
});

const HEX_A = "a".repeat(64);
const HEX_B = "b".repeat(64);
const HEX_C = "c".repeat(64);

function sampleServerEntries() {
  return [
    { platform: "darwin", arch: "arm64", archiveName: "server-1.2.3-darwin-arm64.tar.gz", sha256: HEX_A, size: 111 },
    { platform: "linux", arch: "x64", archiveName: "server-1.2.3-linux-x64.tar.gz", sha256: HEX_B, size: 222 },
  ];
}

function sampleRendererEntry() {
  return { archiveName: "renderer-1.2.3.tar.gz", sha256: HEX_C, size: 333 };
}

function buildSampleManifest(overrides: Partial<Parameters<typeof assembleTrainManifest>[0]> = {}) {
  return assembleTrainManifest({
    version: "1.2.3",
    releasedAt: "2026-07-11T00:00:00.000Z",
    keyId: "2026a",
    channel: "stable",
    train: 1,
    rendererEntry: sampleRendererEntry(),
    serverEntries: sampleServerEntries(),
    mirrors: ["https://github.com/liliMozi/openhanako/releases/download/train-1"],
    rolloutSalt: "test-salt",
    ...overrides,
  });
}

describe("publish-train: parseArgs", () => {
  it("parses tag + channel", () => {
    expect(parseArgs(["--tag", "v1.2.3", "--channel", "stable"])).toEqual({
      tag: "v1.2.3",
      channel: "stable",
      dryRun: false,
      allowPrereleaseStable: false,
    });
  });

  it("parses --dry-run", () => {
    expect(parseArgs(["--tag", "v1.2.3", "--channel", "beta", "--dry-run"])).toEqual({
      tag: "v1.2.3",
      channel: "beta",
      dryRun: true,
      allowPrereleaseStable: false,
    });
  });

  it("parses an explicit prerelease-to-stable override for deliberate manual publishes", () => {
    expect(parseArgs([
      "--tag", "v1.2.3", "--channel", "stable", "--allow-prerelease-stable",
    ])).toEqual({
      tag: "v1.2.3",
      channel: "stable",
      dryRun: false,
      allowPrereleaseStable: true,
    });
  });

  it("requires --tag", () => {
    expect(() => parseArgs(["--channel", "stable"])).toThrow(/--tag is required/);
  });

  it("rejects an unknown channel", () => {
    expect(() => parseArgs(["--tag", "v1.2.3", "--channel", "canary"])).toThrow(/must be "stable" or "beta"/);
  });

  it("rejects unknown flags", () => {
    expect(() => parseArgs(["--tag", "v1.2.3", "--channel", "stable", "--nope"])).toThrow(/unknown argument/);
  });

  it("rejects the stable-only override on beta", () => {
    expect(() => parseArgs([
      "--tag", "v1.2.3", "--channel", "beta", "--allow-prerelease-stable",
    ])).toThrow(/only valid with --channel stable/);
  });
});

describe("publish-train: automatic workflow channel policy", () => {
  it("gates the automatic stable step on a non-prerelease source while beta remains available", () => {
    const workflow = fs.readFileSync(path.join(process.cwd(), ".github", "workflows", "build.yml"), "utf8");
    const stableStart = workflow.indexOf("- name: Publish stable train");
    const betaStart = workflow.indexOf("- name: Publish beta train");

    expect(workflow).toContain("id: source_release");
    expect(stableStart).toBeGreaterThan(-1);
    expect(betaStart).toBeGreaterThan(stableStart);
    expect(workflow.slice(stableStart, betaStart)).toContain(
      "if: steps.source_release.outputs.is_prerelease == 'false'",
    );
  });

  it("requires an explicit, default-off confirmation before the manual workflow passes the stable override", () => {
    const workflow = fs.readFileSync(
      path.join(process.cwd(), ".github", "workflows", "publish-train.yml"),
      "utf8",
    );
    const publisher = fs.readFileSync(path.join(process.cwd(), "scripts", "publish-train.mjs"), "utf8");
    const confirmationStart = workflow.indexOf("allow_prerelease_stable:");
    const publishStart = workflow.indexOf("- name: Publish train");

    expect(confirmationStart).toBeGreaterThan(-1);
    expect(workflow.slice(confirmationStart, publishStart)).toContain("default: false");
    expect(workflow).toContain("ALLOW_PRERELEASE_STABLE: ${{ inputs.allow_prerelease_stable }}");
    const confirmationCondition = 'if [[ "$ALLOW_PRERELEASE_STABLE" == "true" ]]; then';
    const conditionStart = workflow.indexOf(confirmationCondition);
    expect(conditionStart).toBeGreaterThan(-1);
    expect(workflow.slice(conditionStart, workflow.indexOf("fi", conditionStart))).toContain(
      "args+=(--allow-prerelease-stable)",
    );
    expect(workflow).toContain('node scripts/publish-train.mjs "${args[@]}"');
    expect(workflow).not.toContain("inputs.allow_prerelease_stable && '--allow-prerelease-stable'");
    expect(publisher).not.toContain("GITHUB_WORKFLOW_REF");
  });
});

describe("publish-train: archive filename parsing", () => {
  it("parses a server archive name", () => {
    expect(parseServerArchiveName("server-0.385.14-darwin-arm64.tar.gz")).toEqual({
      version: "0.385.14",
      platform: "darwin",
      arch: "arm64",
    });
    expect(parseServerArchiveName("server-0.385.14-win32-x64.tar.gz")).toEqual({
      version: "0.385.14",
      platform: "win32",
      arch: "x64",
    });
  });

  it("returns null for names that don't match", () => {
    expect(parseServerArchiveName("renderer-0.385.14.tar.gz")).toBeNull();
    expect(parseServerArchiveName("server-0.385.14-darwin-arm64.zip")).toBeNull();
  });

  it("parses a renderer archive name", () => {
    expect(parseRendererArchiveName("renderer-0.385.14.tar.gz")).toEqual({ version: "0.385.14" });
  });

  it("returns null for a non-renderer name", () => {
    expect(parseRendererArchiveName("server-0.385.14-darwin-arm64.tar.gz")).toBeNull();
  });
});

describe("publish-train: computeNextTrain (anti-rollback)", () => {
  it("starts at 1 when the channel has never published", () => {
    expect(computeNextTrain(null)).toBe(1);
    expect(computeNextTrain(undefined)).toBe(1);
  });

  it("increments the existing train", () => {
    expect(computeNextTrain({ train: 5 })).toBe(6);
  });

  it("hard-errors on an unparseable train field instead of guessing", () => {
    expect(() => computeNextTrain({ train: "five" as unknown as number })).toThrow(/unparseable train field/);
    expect(() => computeNextTrain({ train: -1 })).toThrow(/unparseable train field/);
    expect(() => computeNextTrain({ train: 1.5 })).toThrow(/unparseable train field/);
  });
});

describe("publish-train: computeMirrors", () => {
  it("builds a channel-scoped release identity and matching GitHub download base", () => {
    expect(computeTrainReleaseTag({ channel: "stable", train: 6 })).toBe("train-stable-6");
    expect(computeTrainReleaseTag({ channel: "beta", train: 6 })).toBe("train-beta-6");
    expect(computeMirrors({ repo: "liliMozi/openhanako", channel: "beta", train: 6 })).toEqual([
      "https://github.com/liliMozi/openhanako/releases/download/train-beta-6",
    ]);
  });

  it("rejects a malformed repo", () => {
    expect(() => computeMirrors({ repo: "openhanako", channel: "stable", train: 1 })).toThrow(/repo must be/);
  });

  it("rejects a non-positive train", () => {
    expect(() => computeMirrors({ repo: "liliMozi/openhanako", channel: "stable", train: 0 })).toThrow(/train must be a positive integer/);
  });

  it("rejects an unknown channel before constructing a release tag", () => {
    expect(() => computeTrainReleaseTag({ channel: "canary", train: 1 })).toThrow(/channel must be/);
  });
});

describe("publish-train: assembleTrainManifest (reuses buildSeedManifest, no parallel builder)", () => {
  it("merges per-platform server entries under one manifest carrying contract from buildSeedManifest", () => {
    const manifest = buildSampleManifest();
    expect(manifest.schema).toBe(1);
    expect(manifest.train).toBe(1);
    expect(manifest.channel).toBe("stable");
    expect(manifest.contract).toEqual({ preload: 1, serverProtocol: 1 }); // sourced from buildSeedManifest, not invented here
    expect(manifest.rollout).toEqual({ percent: 100, salt: "test-salt" });
    expect(manifest.mirrors).toEqual(["https://github.com/liliMozi/openhanako/releases/download/train-1"]);
    expect(Object.keys(manifest.artifacts.server).sort()).toEqual(["darwin-arm64", "linux-x64"]);
    expect(manifest.artifacts.server["darwin-arm64"]).toEqual({
      version: "1.2.3", sha256: HEX_A, size: 111, path: "server-1.2.3-darwin-arm64.tar.gz",
    });
    expect(manifest.artifacts.renderer).toEqual({
      version: "1.2.3", sha256: HEX_C, size: 333, path: "renderer-1.2.3.tar.gz",
    });
  });

  it("sets minShell to the hand-maintained SHELL_COMPAT_FLOOR, never the train's own version", () => {
    // sampleServerEntries()/sampleRendererEntry() are pinned to version "1.2.3",
    // which is deliberately not equal to SHELL_COMPAT_FLOOR: if the
    // implementation ever regresses to `minShell: version` (buildSeedManifest's
    // seed-only convention), this test must fail rather than pass by
    // coincidence.
    expect(SHELL_COMPAT_FLOOR).not.toBe("1.2.3");
    const manifest = buildSampleManifest();
    expect(manifest.minShell).toBe(SHELL_COMPAT_FLOOR);
    expect(manifest.minShell).not.toBe("1.2.3");
  });

  it("passes manifestModule.validateManifest (self-checked before signing)", () => {
    expect(() => buildSampleManifest()).not.toThrow();
  });

  it("refuses to build a manifest with zero server entries", () => {
    expect(() => buildSampleManifest({ serverEntries: [] })).toThrow(/at least one server artifact entry/);
  });
});

describe("publish-train: diffManifestArtifacts / assertArtifactsMatchForResume", () => {
  it("reports a match when artifact entries agree, even if channel/releasedAt/salt differ", () => {
    const a = buildSampleManifest({ channel: "stable", releasedAt: "2026-07-11T00:00:00.000Z", rolloutSalt: "s1" });
    const b = buildSampleManifest({ channel: "beta", releasedAt: "2026-07-11T00:05:00.000Z", rolloutSalt: "s2" });
    expect(diffManifestArtifacts(a, b)).toEqual({ matches: true, mismatches: [] });
    expect(() => assertArtifactsMatchForResume(a, b)).not.toThrow();
  });

  it("reports a mismatch when a server archive's sha256 differs", () => {
    const a = buildSampleManifest();
    const differentEntries = sampleServerEntries();
    differentEntries[0] = { ...differentEntries[0], sha256: "f".repeat(64) };
    const b = buildSampleManifest({ serverEntries: differentEntries });
    const diff = diffManifestArtifacts(a, b);
    expect(diff.matches).toBe(false);
    expect(diff.mismatches[0]).toMatch(/server\.darwin-arm64/);
  });

  it("treats artifact size and path as part of resumable-publish identity", () => {
    const a = buildSampleManifest();
    const b = structuredClone(a);
    b.artifacts.renderer.size += 1;
    b.artifacts.server["darwin-arm64"].path = "renamed-server.tar.gz";

    const diff = diffManifestArtifacts(a, b);
    expect(diff.matches).toBe(false);
    expect(diff.mismatches.join("\n")).toMatch(/renderer.*size/);
    expect(diff.mismatches.join("\n")).toMatch(/server\.darwin-arm64.*path/);
  });

  it("throws with the mismatch details baked into the message (real conflict, hard error)", () => {
    const a = buildSampleManifest();
    const b = buildSampleManifest({ rendererEntry: { ...sampleRendererEntry(), sha256: "d".repeat(64) } });
    expect(() => assertArtifactsMatchForResume(a, b)).toThrow(/real conflict, not a safe resume/);
    expect(() => assertArtifactsMatchForResume(a, b)).toThrow(/renderer:/);
  });
});

describe("publish-train: discoverBoxes", () => {
  function seedDownloadedBoxes(dir: string, version: string) {
    fs.writeFileSync(path.join(dir, `server-${version}-darwin-arm64.tar.gz`), "server-mac-bytes");
    fs.writeFileSync(path.join(dir, `server-${version}-linux-x64.tar.gz`), "server-linux-bytes");
    fs.writeFileSync(path.join(dir, `renderer-${version}.tar.gz`), "renderer-bytes");
  }

  it("parses and hashes every downloaded archive", async () => {
    const dir = makeTempDir("hana-discover-boxes-");
    seedDownloadedBoxes(dir, "1.2.3");
    const downloadAssets = vi.fn();
    const result = await discoverBoxes({
      tag: "v1.2.3",
      workDir: dir,
      deps: {
        downloadAssets,
        readdir: (p: string) => fs.readdirSync(p),
        sha256File: async (p: string) => createHash("sha256").update(fs.readFileSync(p)).digest("hex"),
        statSize: (p: string) => fs.statSync(p).size,
      },
    });
    expect(downloadAssets).toHaveBeenCalledWith("v1.2.3", ["server-*.tar.gz", "renderer-*.tar.gz"], dir);
    expect(result.serverEntries).toHaveLength(2);
    expect(result.serverEntries.map((e: { platform: string }) => e.platform).sort()).toEqual(["darwin", "linux"]);
    expect(result.rendererEntry.archiveName).toBe("renderer-1.2.3.tar.gz");
    expect(result.rendererEntry.sha256).toHaveLength(64);
  });

  it("hard-errors when more than one renderer archive is present", async () => {
    const dir = makeTempDir("hana-discover-boxes-");
    seedDownloadedBoxes(dir, "1.2.3");
    fs.writeFileSync(path.join(dir, "renderer-1.2.3-dup.tar.gz"), "dup");
    await expect(
      discoverBoxes({
        tag: "v1.2.3",
        workDir: dir,
        deps: {
          downloadAssets: vi.fn(),
          readdir: (p: string) => fs.readdirSync(p),
          sha256File: async () => HEX_A,
          statSize: () => 1,
        },
      }),
    ).rejects.toThrow(/expected exactly one renderer archive/);
  });

  it("hard-errors when an archive's embedded version doesn't match the release tag", async () => {
    const dir = makeTempDir("hana-discover-boxes-");
    seedDownloadedBoxes(dir, "9.9.9"); // stale version vs the v1.2.3 tag below
    await expect(
      discoverBoxes({
        tag: "v1.2.3",
        workDir: dir,
        deps: {
          downloadAssets: vi.fn(),
          readdir: (p: string) => fs.readdirSync(p),
          sha256File: async () => HEX_A,
          statSize: () => 1,
        },
      }),
    ).rejects.toThrow(/does not match|expected 1\.2\.3/);
  });
});

describe("publish-train: releaseExistsFromExec (only a real not-found reads as absent)", () => {
  // Error shape matches what execFileSync throws for a failed gh call with
  // encoding "utf8" and piped stdio: message "Command failed: ..." plus the
  // captured stderr as a string property.
  function makeExecError(stderr: string) {
    const err = new Error(`Command failed: gh release view some-tag --json tagName\n${stderr}`);
    (err as Error & { stderr: string }).stderr = stderr;
    (err as Error & { status: number }).status = 1;
    return err;
  }

  it("returns true when the gh call succeeds", () => {
    expect(releaseExistsFromExec("train-1", () => '{"tagName":"train-1"}')).toBe(true);
  });

  it('returns false on the exact "release not found" stderr gh emits for a missing release', () => {
    // Measured against gh 2.92.0: `gh release view <missing-tag> --json tagName`
    // exits 1 with stderr "release not found\n".
    const exec = vi.fn(() => {
      throw makeExecError("release not found\n");
    });
    expect(releaseExistsFromExec("train-1", exec)).toBe(false);
  });

  it("rethrows auth/network/rate-limit failures instead of reading them as absent", () => {
    for (const stderr of [
      "HTTP 401: Bad credentials (https://api.github.com/graphql)\n",
      "HTTP 403: API rate limit exceeded\n",
      "error connecting to api.github.com\n",
    ]) {
      const exec = vi.fn(() => {
        throw makeExecError(stderr);
      });
      expect(() => releaseExistsFromExec("train-1", exec)).toThrow(/gh release view train-1 failed/);
    }
  });

  it("rethrows errors that carry no stderr at all (spawn failure, gh missing)", () => {
    const exec = vi.fn(() => {
      throw new Error("spawn gh ENOENT");
    });
    expect(() => releaseExistsFromExec("train-1", exec)).toThrow(/spawn gh ENOENT/);
  });
});

describe("publish-train: source release metadata", () => {
  it("reads the exact release classification fields for the requested tag", () => {
    const exec = vi.fn().mockReturnValue(JSON.stringify({
      tagName: "v1.2.3",
      isDraft: false,
      isPrerelease: true,
    }));

    expect(releaseInfoFromExec("v1.2.3", exec)).toEqual({
      tagName: "v1.2.3",
      isDraft: false,
      isPrerelease: true,
    });
    expect(exec).toHaveBeenCalledWith([
      "release", "view", "v1.2.3", "--json", "tagName,isDraft,isPrerelease",
    ]);
  });

  it("hard-errors on malformed or mismatched metadata", () => {
    expect(() => releaseInfoFromExec("v1.2.3", () => "not-json")).toThrow(/could not read source release metadata/);
    expect(() => releaseInfoFromExec("v1.2.3", () => JSON.stringify({
      tagName: "v9.9.9",
      isDraft: false,
      isPrerelease: false,
    }))).toThrow(/malformed or names a different tag/);
  });
});

describe("publish-train: publishChannel", () => {
  function baseDeps(overrides: Record<string, unknown> = {}) {
    return {
      releaseInfo: vi.fn().mockReturnValue({
        tagName: "v1.2.3",
        isDraft: false,
        isPrerelease: false,
      }),
      releaseExists: vi.fn().mockReturnValue(false),
      releaseAssetNames: vi.fn().mockReturnValue([]),
      downloadAssets: vi.fn(),
      createRelease: vi.fn(),
      uploadAssets: vi.fn(),
      signManifest: vi.fn(),
      mkdtemp: (prefix: string) => makeTempDir(prefix),
      writeFile: (p: string, data: string | Buffer) => fs.writeFileSync(p, data),
      readFile: (p: string) => fs.readFileSync(p),
      verifyManifest: vi.fn((manifestBytes: Buffer | string) => JSON.parse(Buffer.from(manifestBytes).toString("utf8"))),
      randomSalt: () => "fixed-salt",
      log: () => {},
      ...overrides,
    };
  }

  const boxes = { serverEntries: sampleServerEntries(), rendererEntry: sampleRendererEntry() };

  function makeSigningEnv() {
    const dir = makeTempDir("hana-publish-train-signing-");
    const signKeyPath = path.join(dir, "test-signing-key.pem");
    fs.writeFileSync(signKeyPath, "test signing key fixture");
    return { HANA_SIGN_KEY: signKeyPath };
  }

  it("refuses to advance stable from a prerelease before any publish write", async () => {
    const deps = baseDeps({
      releaseInfo: vi.fn().mockReturnValue({
        tagName: "v1.2.3",
        isDraft: false,
        isPrerelease: true,
      }),
    });

    await expect(
      publishChannel({
        tag: "v1.2.3", channel: "stable", dryRun: false, repo: "liliMozi/openhanako",
        releasedAt: "2026-07-11T00:00:00.000Z", boxes, env: makeSigningEnv(), deps, log: () => {},
      }),
    ).rejects.toThrow(/prerelease.*beta/i);

    expect(deps.releaseExists).not.toHaveBeenCalled();
    expect(deps.createRelease).not.toHaveBeenCalled();
    expect(deps.uploadAssets).not.toHaveBeenCalled();
    expect(deps.signManifest).not.toHaveBeenCalled();
  });

  it("refuses to publish any channel from a draft release", async () => {
    const deps = baseDeps({
      releaseInfo: vi.fn().mockReturnValue({
        tagName: "v1.2.3",
        isDraft: true,
        isPrerelease: true,
      }),
    });

    await expect(
      publishChannel({
        tag: "v1.2.3", channel: "beta", dryRun: false, repo: "liliMozi/openhanako",
        releasedAt: "2026-07-11T00:00:00.000Z", boxes, env: makeSigningEnv(), deps, log: () => {},
      }),
    ).rejects.toThrow(/still a draft/);

    expect(deps.releaseExists).not.toHaveBeenCalled();
    expect(deps.createRelease).not.toHaveBeenCalled();
  });

  it("allows a prerelease to advance beta", async () => {
    const deps = baseDeps({
      releaseInfo: vi.fn().mockReturnValue({
        tagName: "v1.2.3",
        isDraft: false,
        isPrerelease: true,
      }),
    });

    const result = await publishChannel({
      tag: "v1.2.3", channel: "beta", dryRun: false, repo: "liliMozi/openhanako",
      releasedAt: "2026-07-11T00:00:00.000Z", boxes, env: makeSigningEnv(), deps, log: () => {},
    });

    expect(result).toEqual({ action: "created", channel: "beta", train: 1, trainTag: "train-beta-1" });
    expect(deps.createRelease).toHaveBeenCalledTimes(2);
  });

  it("allows a deliberate manual stable publish from a prerelease", async () => {
    const deps = baseDeps({
      releaseInfo: vi.fn().mockReturnValue({
        tagName: "v1.2.3",
        isDraft: false,
        isPrerelease: true,
      }),
    });

    const result = await publishChannel({
      tag: "v1.2.3", channel: "stable", dryRun: false, repo: "liliMozi/openhanako",
      releasedAt: "2026-07-11T00:00:00.000Z", boxes, env: makeSigningEnv(), deps, log: () => {},
      allowPrereleaseStable: true,
    });

    expect(result).toEqual({ action: "created", channel: "stable", train: 1, trainTag: "train-stable-1" });
    expect(deps.createRelease).toHaveBeenCalledTimes(2);
  });

  it("dry-run computes and prints the plan but calls zero publish operations", async () => {
    const deps = baseDeps();
    const result = await publishChannel({
      tag: "v1.2.3", channel: "stable", dryRun: true, repo: "liliMozi/openhanako",
      releasedAt: "2026-07-11T00:00:00.000Z", boxes, env: makeSigningEnv(), deps, log: () => {},
    });
    expect(result.action).toBe("dry-run");
    expect(result.train).toBe(1);
    expect(deps.createRelease).not.toHaveBeenCalled();
    expect(deps.uploadAssets).not.toHaveBeenCalled();
    expect(deps.signManifest).not.toHaveBeenCalled();
  });

  it("first-ever publish: creates both the train release and the channels release", async () => {
    const deps = baseDeps(); // releaseExists() -> false for everything: channels AND train-stable-1 are both new
    const result = await publishChannel({
      tag: "v1.2.3", channel: "stable", dryRun: false, repo: "liliMozi/openhanako",
      releasedAt: "2026-07-11T00:00:00.000Z", boxes, env: makeSigningEnv(), deps, log: () => {},
    });
    expect(result).toEqual({ action: "created", channel: "stable", train: 1, trainTag: "train-stable-1" });
    expect(deps.createRelease).toHaveBeenCalledTimes(2);
    expect((deps.createRelease as ReturnType<typeof vi.fn>).mock.calls[0][0]).toBe("train-stable-1");
    expect((deps.createRelease as ReturnType<typeof vi.fn>).mock.calls[1][0]).toBe("channels");
    expect(deps.uploadAssets).not.toHaveBeenCalled();
    expect(deps.signManifest).toHaveBeenCalledTimes(2); // train.json + channel pointer
  });

  it("refuses an incomplete channel pointer instead of deriving anti-rollback state from unsigned JSON", async () => {
    const deps = baseDeps({
      releaseExists: vi.fn((tag: string) => tag === "channels"),
      releaseAssetNames: vi.fn().mockReturnValue(["stable.json"]),
    });

    await expect(publishChannel({
      tag: "v1.2.3", channel: "stable", dryRun: false, repo: "liliMozi/openhanako",
      releasedAt: "2026-07-11T00:00:00.000Z", boxes, env: makeSigningEnv(), deps, log: () => {},
    })).rejects.toThrow(/stable\.json.*signature.*incomplete/i);

    expect(deps.downloadAssets).not.toHaveBeenCalled();
    expect(deps.createRelease).not.toHaveBeenCalled();
    expect(deps.uploadAssets).not.toHaveBeenCalled();
  });

  it("refuses a signed channel pointer whose manifest names another channel", async () => {
    const wrongChannel = buildSampleManifest({ version: "1.2.2", train: 13, channel: "beta" });
    const deps = baseDeps({
      releaseExists: vi.fn((tag: string) => tag === "channels"),
      releaseAssetNames: vi.fn().mockReturnValue(["stable.json", "stable.json.sig"]),
      downloadAssets: vi.fn((tag: string, _patterns: string[], destDir: string) => {
        if (tag === "channels") writeSignedManifestFixture(destDir, "stable.json", wrongChannel);
      }),
    });

    await expect(publishChannel({
      tag: "v1.2.3", channel: "stable", dryRun: false, repo: "liliMozi/openhanako",
      releasedAt: "2026-07-11T00:00:00.000Z", boxes, env: makeSigningEnv(), deps, log: () => {},
    })).rejects.toThrow(/pointer.*channel.*beta.*stable/i);

    expect(deps.createRelease).not.toHaveBeenCalled();
    expect(deps.uploadAssets).not.toHaveBeenCalled();
  });

  it("migrates a legacy pointer to a channel-scoped release even when the other channel already owns that train number", async () => {
    // Real divergence shape: stable last published train 13 using the legacy
    // global train-N layout, while beta has already consumed global trains 14
    // and 15 with different boxes. Stable must still be able to advance to its
    // own train 14 without inspecting or colliding with beta's old train-14.
    const priorStable = buildSampleManifest({
      version: "1.2.2",
      train: 13,
      channel: "stable",
      mirrors: ["https://github.com/liliMozi/openhanako/releases/download/train-13"],
    });
    const betaTrain14 = buildSampleManifest({
      version: "1.2.1",
      train: 14,
      channel: "beta",
      rendererEntry: { ...sampleRendererEntry(), sha256: "9".repeat(64) },
      mirrors: ["https://github.com/liliMozi/openhanako/releases/download/train-14"],
    });
    let publishedPointerBytes: Buffer | null = null;
    const deps = baseDeps({
      releaseExists: vi.fn((tag: string) => tag === "channels" || tag === "train-14" || tag === "train-15"),
      releaseAssetNames: vi.fn().mockReturnValue([
        "stable.json", "stable.json.sig", "beta.json", "beta.json.sig",
      ]),
      downloadAssets: vi.fn((tag: string, _patterns: string[], destDir: string) => {
        if (tag === "channels") {
          writeSignedManifestFixture(destDir, "stable.json", priorStable);
        } else if (tag === "train-14") {
          writeSignedManifestFixture(destDir, "train.json", betaTrain14);
        }
      }),
      writeFile: (p: string, data: string | Buffer) => {
        if (path.basename(p) === "stable.json") publishedPointerBytes = Buffer.from(data);
        fs.writeFileSync(p, data);
      },
    });

    const result = await publishChannel({
      tag: "v1.2.3", channel: "stable", dryRun: false, repo: "liliMozi/openhanako",
      releasedAt: "2026-07-11T00:00:00.000Z", boxes, env: makeSigningEnv(), deps, log: () => {},
    });

    expect(result).toEqual({
      action: "created", channel: "stable", train: 14, trainTag: "train-stable-14",
    });
    expect(deps.releaseExists).toHaveBeenCalledWith("train-stable-14");
    expect(deps.releaseExists).not.toHaveBeenCalledWith("train-14");
    expect(deps.createRelease).toHaveBeenCalledWith(
      "train-stable-14",
      expect.any(Array),
      expect.objectContaining({ title: "train-stable-14" }),
    );

    expect(publishedPointerBytes).not.toBeNull();
    const published = JSON.parse((publishedPointerBytes as unknown as Buffer).toString("utf8"));
    expect(published.train).toBe(14);
    expect(published.channel).toBe("stable");
    expect(published.mirrors).toEqual([
      "https://github.com/liliMozi/openhanako/releases/download/train-stable-14",
    ]);
  });

  it("is a no-op when the channel pointer already publishes the same source artifacts", async () => {
    const alreadyPublished = buildSampleManifest({
      train: 13,
      channel: "stable",
      mirrors: ["https://github.com/liliMozi/openhanako/releases/download/train-13"],
    });
    const deps = baseDeps({
      releaseExists: vi.fn((tag: string) => tag === "channels"),
      releaseAssetNames: vi.fn().mockReturnValue(["stable.json", "stable.json.sig"]),
      downloadAssets: vi.fn((tag: string, _patterns: string[], destDir: string) => {
        if (tag === "channels") {
          writeSignedManifestFixture(destDir, "stable.json", alreadyPublished);
        }
      }),
    });

    const result = await publishChannel({
      tag: "v1.2.3", channel: "stable", dryRun: false, repo: "liliMozi/openhanako",
      releasedAt: "2026-07-11T00:00:00.000Z", boxes, env: makeSigningEnv(), deps, log: () => {},
    });

    expect(result).toEqual({ action: "already-published", channel: "stable", train: 13, trainTag: null });
    expect(deps.releaseExists).toHaveBeenCalledTimes(1);
    expect(deps.createRelease).not.toHaveBeenCalled();
    expect(deps.uploadAssets).not.toHaveBeenCalled();
    expect(deps.signManifest).not.toHaveBeenCalled();
  });

  it("refuses to advance a channel when the same content version has different bytes", async () => {
    const mutatedSameVersion = buildSampleManifest({
      train: 13,
      channel: "stable",
      rendererEntry: { ...sampleRendererEntry(), sha256: "9".repeat(64) },
    });
    const deps = baseDeps({
      releaseExists: vi.fn((tag: string) => tag === "channels"),
      releaseAssetNames: vi.fn().mockReturnValue(["stable.json", "stable.json.sig"]),
      downloadAssets: vi.fn((tag: string, _patterns: string[], destDir: string) => {
        if (tag === "channels") {
          writeSignedManifestFixture(destDir, "stable.json", mutatedSameVersion);
        }
      }),
    });

    await expect(publishChannel({
      tag: "v1.2.3", channel: "stable", dryRun: false, repo: "liliMozi/openhanako",
      releasedAt: "2026-07-11T00:00:00.000Z", boxes, env: makeSigningEnv(), deps, log: () => {},
    })).rejects.toThrow(/same content version.*different artifacts/i);

    expect(deps.createRelease).not.toHaveBeenCalled();
    expect(deps.uploadAssets).not.toHaveBeenCalled();
    expect(deps.signManifest).not.toHaveBeenCalled();
  });

  it("channels release already exists (another channel published before): uploads instead of creating it", async () => {
    const deps = baseDeps({
      releaseExists: vi.fn((tag: string) => tag === "channels"), // channels exists, train-beta-1 does not
      releaseAssetNames: vi.fn().mockReturnValue(["stable.json", "stable.json.sig"]), // beta not published yet
    });
    const result = await publishChannel({
      tag: "v1.2.3", channel: "beta", dryRun: false, repo: "liliMozi/openhanako",
      releasedAt: "2026-07-11T00:00:00.000Z", boxes, env: makeSigningEnv(), deps, log: () => {},
    });
    expect(result.action).toBe("created");
    expect(deps.createRelease).toHaveBeenCalledTimes(1); // only train-beta-1 (channels already exists)
    expect(deps.createRelease).toHaveBeenCalledWith("train-beta-1", expect.any(Array), expect.objectContaining({ title: "train-beta-1" }));
    expect(deps.uploadAssets).toHaveBeenCalledTimes(1);
    expect(deps.uploadAssets).toHaveBeenCalledWith("channels", expect.any(Array));
  });

  it("resumes when the same channel's scoped train release already exists with byte-identical artifacts", async () => {
    // Simulate: stable's own last train was 5 and an interrupted stable run
    // already created train-stable-6 with the candidate boxes.
    const priorManifest = buildSampleManifest({ version: "1.2.2", train: 5, channel: "stable" });
    const existingTrain6 = buildSampleManifest({ train: 6, channel: "stable", releasedAt: "2026-07-10T00:00:00.000Z", rolloutSalt: "old-salt" });

    const deps = baseDeps({
      releaseExists: vi.fn((tag: string) => tag === "channels" || tag === "train-stable-6"),
      releaseAssetNames: vi.fn().mockReturnValue(["stable.json", "stable.json.sig"]),
      downloadAssets: vi.fn((tag: string, _patterns: string[], destDir: string) => {
        if (tag === "channels") {
          writeSignedManifestFixture(destDir, "stable.json", priorManifest);
        } else if (tag === "train-stable-6") {
          writeSignedManifestFixture(destDir, "train.json", existingTrain6);
        }
      }),
    });

    const result = await publishChannel({
      tag: "v1.2.3", channel: "stable", dryRun: false, repo: "liliMozi/openhanako",
      releasedAt: "2026-07-11T00:00:00.000Z", boxes, env: makeSigningEnv(), deps, log: () => {},
    });

    expect(result).toEqual({ action: "resumed", channel: "stable", train: 6, trainTag: "train-stable-6" });
    expect(deps.createRelease).not.toHaveBeenCalled(); // train-stable-6 box upload skipped
    expect(deps.uploadAssets).toHaveBeenCalledTimes(1); // pointer still gets updated
    expect(deps.uploadAssets).toHaveBeenCalledWith("channels", expect.any(Array));
    expect(deps.signManifest).toHaveBeenCalledTimes(1); // only the fresh channel pointer, not train.json (reused as-is)
  });

  it("refuses to resume from a scoped train manifest whose signature fails verification", async () => {
    const priorManifest = buildSampleManifest({ version: "1.2.2", train: 5, channel: "stable" });
    const existingTrain6 = buildSampleManifest({ train: 6, channel: "stable" });
    const deps = baseDeps({
      releaseExists: vi.fn((tag: string) => tag === "channels" || tag === "train-stable-6"),
      releaseAssetNames: vi.fn().mockReturnValue(["stable.json", "stable.json.sig"]),
      downloadAssets: vi.fn((tag: string, _patterns: string[], destDir: string) => {
        if (tag === "channels") writeSignedManifestFixture(destDir, "stable.json", priorManifest);
        if (tag === "train-stable-6") writeSignedManifestFixture(destDir, "train.json", existingTrain6);
      }),
      verifyManifest: vi.fn((manifestBytes: Buffer | string) => {
        const manifest = JSON.parse(Buffer.from(manifestBytes).toString("utf8"));
        if (manifest.train === 6) throw new Error("manifest: signature verification failed");
        return manifest;
      }),
    });

    await expect(publishChannel({
      tag: "v1.2.3", channel: "stable", dryRun: false, repo: "liliMozi/openhanako",
      releasedAt: "2026-07-11T00:00:00.000Z", boxes, env: makeSigningEnv(), deps, log: () => {},
    })).rejects.toThrow(/signature verification failed/);

    expect(deps.uploadAssets).not.toHaveBeenCalled();
    expect(deps.signManifest).not.toHaveBeenCalled();
  });

  it("hard-errors when the same channel's scoped train release exists with DIFFERENT artifacts", async () => {
    const priorManifest = buildSampleManifest({ version: "1.2.2", train: 5, channel: "stable" });
    const conflictingEntries = sampleServerEntries();
    conflictingEntries[0] = { ...conflictingEntries[0], sha256: "9".repeat(64) };
    const existingTrain6 = buildSampleManifest({ train: 6, channel: "stable", serverEntries: conflictingEntries });

    const deps = baseDeps({
      releaseExists: vi.fn((tag: string) => tag === "channels" || tag === "train-stable-6"),
      releaseAssetNames: vi.fn().mockReturnValue(["stable.json", "stable.json.sig"]),
      downloadAssets: vi.fn((tag: string, _patterns: string[], destDir: string) => {
        if (tag === "channels") {
          writeSignedManifestFixture(destDir, "stable.json", priorManifest);
        } else if (tag === "train-stable-6") {
          writeSignedManifestFixture(destDir, "train.json", existingTrain6);
        }
      }),
    });

    await expect(
      publishChannel({
        tag: "v1.2.3", channel: "stable", dryRun: false, repo: "liliMozi/openhanako",
        releasedAt: "2026-07-11T00:00:00.000Z", boxes, env: makeSigningEnv(), deps, log: () => {},
      }),
    ).rejects.toThrow(/real conflict, not a safe resume/);
    expect(deps.createRelease).not.toHaveBeenCalled();
    expect(deps.uploadAssets).not.toHaveBeenCalled();
  });

  it("propagates a non-not-found releaseExists failure and makes zero publish calls", async () => {
    const deps = baseDeps({
      releaseExists: vi.fn(() => {
        throw new Error("gh release view channels failed: HTTP 401: Bad credentials");
      }),
    });
    await expect(
      publishChannel({
        tag: "v1.2.3", channel: "stable", dryRun: false, repo: "liliMozi/openhanako",
        releasedAt: "2026-07-11T00:00:00.000Z", boxes, env: makeSigningEnv(), deps, log: () => {},
      }),
    ).rejects.toThrow(/HTTP 401/);
    expect(deps.createRelease).not.toHaveBeenCalled();
    expect(deps.uploadAssets).not.toHaveBeenCalled();
    expect(deps.signManifest).not.toHaveBeenCalled();
  });

  it("beta.json keeps channel:\"beta\" when resuming beta's own scoped release", async () => {
    const priorBetaManifest = buildSampleManifest({ version: "1.2.2", train: 5, channel: "beta" });
    const existingBetaTrain6 = buildSampleManifest({ train: 6, channel: "beta" });

    let publishedPointerBytes: Buffer | null = null;
    const deps = baseDeps({
      releaseExists: vi.fn((tag: string) => tag === "channels" || tag === "train-beta-6"),
      releaseAssetNames: vi.fn().mockReturnValue(["beta.json", "beta.json.sig", "stable.json", "stable.json.sig"]),
      downloadAssets: vi.fn((tag: string, _patterns: string[], destDir: string) => {
        if (tag === "channels") {
          writeSignedManifestFixture(destDir, "beta.json", priorBetaManifest);
        } else if (tag === "train-beta-6") {
          writeSignedManifestFixture(destDir, "train.json", existingBetaTrain6);
        }
      }),
      writeFile: (p: string, data: string | Buffer) => {
        if (path.basename(p) === "beta.json") publishedPointerBytes = Buffer.from(data);
        fs.writeFileSync(p, data);
      },
    });

    const result = await publishChannel({
      tag: "v1.2.3", channel: "beta", dryRun: false, repo: "liliMozi/openhanako",
      releasedAt: "2026-07-11T00:00:00.000Z", boxes, env: makeSigningEnv(), deps, log: () => {},
    });

    expect(result.action).toBe("resumed");
    expect(publishedPointerBytes).not.toBeNull();
    const published = JSON.parse((publishedPointerBytes as unknown as Buffer).toString("utf8"));
    expect(published.channel).toBe("beta");
    expect(published.train).toBe(6);
  });
});
