import { describe, expect, it } from "vitest";

import { evaluateReleasePreflight, runReleasePreflight } from "../scripts/release-preflight.mjs";

const legacyHistory = [
  { tag: "v0.1.2", version: "0.1.2" },
  { tag: "v0.1.21", version: "0.1.21" },
  { tag: "v0.1.22", version: "0.1.22" },
];

describe("release preflight", () => {
  it("reproduces and rejects the 0.1.22 → 0.1.3 version regression", () => {
    expect(() => evaluateReleasePreflight({
      candidateTag: "v0.1.3",
      candidateVersion: "0.1.3",
      candidateGeneration: 1,
      historicalReleases: legacyHistory,
    })).toThrow(/0\.1\.3.*not greater.*0\.1\.22/i);
  });

  it("accepts the monotonic 0.1.23 repair release", () => {
    expect(evaluateReleasePreflight({
      candidateTag: "v0.1.23",
      candidateVersion: "0.1.23",
      candidateGeneration: 1,
      historicalReleases: legacyHistory,
    })).toMatchObject({
      candidateVersion: "0.1.23",
      historicalMaximumVersion: "0.1.22",
      historicalMaximumGeneration: 0,
    });
  });

  it("rejects a tag/package mismatch and an invalid SemVer", () => {
    expect(() => evaluateReleasePreflight({
      candidateTag: "v0.1.24",
      candidateVersion: "0.1.23",
      candidateGeneration: 1,
      historicalReleases: legacyHistory,
    })).toThrow(/does not match package version/i);
    expect(() => evaluateReleasePreflight({
      candidateTag: "vbanana",
      candidateVersion: "banana",
      candidateGeneration: 1,
      historicalReleases: [],
    })).toThrow(/not valid SemVer/i);
  });

  it("uses standard prerelease ordering", () => {
    expect(evaluateReleasePreflight({
      candidateTag: "v0.1.24-beta.1",
      candidateVersion: "0.1.24-beta.1",
      candidateGeneration: 2,
      historicalReleases: [...legacyHistory, { tag: "v0.1.23", version: "0.1.23", releaseGeneration: 1 }],
    }).historicalMaximumVersion).toBe("0.1.23");
  });

  it("rejects release generation reuse even when product version increased", () => {
    expect(() => evaluateReleasePreflight({
      candidateTag: "v0.1.24",
      candidateVersion: "0.1.24",
      candidateGeneration: 1,
      historicalReleases: [...legacyHistory, { tag: "v0.1.23", version: "0.1.23", releaseGeneration: 1 }],
    })).toThrow(/releaseGeneration 1.*greater.*1/i);
  });

  it("passes against all fetched tags that belong to the current product", () => {
    expect(runReleasePreflight({ candidateTag: "v0.1.35" })).toMatchObject({
      candidateVersion: "0.1.35",
      historicalMaximumVersion: "0.1.34",
      historicalMaximumGeneration: 12,
      candidateGeneration: 13,
    });
  });
});
