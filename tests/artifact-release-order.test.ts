import { createRequire } from "node:module";
import { describe, expect, it } from "vitest";

const require = createRequire(import.meta.url);
const releaseOrder = require("../shared/artifact-core/release-order.cjs");
const { buildArtifactRuntimeProvenance } = require("../desktop/src/shared/artifact-runtime-provenance.cjs");

describe("artifact release order", () => {
  it("orders standard SemVer including prereleases numerically", () => {
    expect(releaseOrder.compareProductVersions("0.1.22", "0.1.3")).toBeGreaterThan(0);
    expect(releaseOrder.compareProductVersions("0.1.24-beta.1", "0.1.23")).toBeGreaterThan(0);
    expect(releaseOrder.compareProductVersions("0.1.24-beta.1", "0.1.24")).toBeLessThan(0);
  });

  it("makes a generation-bearing release newer than legacy metadata and blocks the reverse", () => {
    expect(releaseOrder.compareReleaseOrder(
      { version: "0.1.23", releaseGeneration: 1 },
      { version: "0.1.22" },
    )).toBe(1);
    expect(releaseOrder.compareReleaseOrder(
      { version: "9.9.9" },
      { version: "0.1.23", releaseGeneration: 1 },
    )).toBe(-1);
  });

  it("warns on mixed runtime generations without disabling independent rollback", () => {
    expect(releaseOrder.assessRuntimeCompatibility(
      { version: "0.1.23", releaseGeneration: 1 },
      { version: "0.1.24", releaseGeneration: 2 },
    )).toEqual({ status: "warning", reason: "mixed-release-generation" });
  });
});

describe("artifact runtime provenance", () => {
  it("reports shell, server, renderer, train, source, commit, hash and compatibility", () => {
    const record = buildArtifactRuntimeProvenance({
      shellVersion: "0.1.23",
      seed: { releaseGeneration: 1, sourceCommit: "a".repeat(40), releasedAt: "2026-08-09T00:00:00.000Z" },
      server: {
        version: "0.1.23", releaseGeneration: 1, train: 0, sha256: "b".repeat(64),
        slot: "current", source: "seed", sourceCommit: "a".repeat(40),
      },
      renderer: {
        version: "0.1.22", train: 4, sha256: "c".repeat(64),
        slot: "previous", source: "ota", sourceCommit: null,
      },
      compatibility: { status: "warning", reason: "mixed-generation-metadata" },
    });

    expect(record).toMatchObject({
      event: "artifact-runtime-provenance",
      shell: { version: "0.1.23", releaseGeneration: 1 },
      server: { version: "0.1.23", train: 0, source: "seed", slot: "current" },
      renderer: { version: "0.1.22", train: 4, source: "ota", slot: "previous" },
      compatibility: { status: "warning", reason: "mixed-generation-metadata" },
    });
  });
});
