import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import yaml from "js-yaml";
import { describe, expect, it } from "vitest";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

type WorkflowStep = {
  name?: string;
  run?: string;
  with?: Record<string, unknown>;
};

type WorkflowJob = {
  needs?: string | string[];
  steps?: WorkflowStep[];
};

type Workflow = {
  jobs: Record<string, WorkflowJob>;
};

function readWorkflow(name: string): Workflow {
  const value = yaml.load(fs.readFileSync(path.join(ROOT, ".github", "workflows", name), "utf8"));
  if (!value || typeof value !== "object" || !("jobs" in value)) {
    throw new Error(`workflow ${name} is missing jobs`);
  }
  return value as Workflow;
}

function stepText(job: WorkflowJob): string {
  return (job.steps || []).map((step) => String(step.run || "")).join("\n");
}

describe("release workflow hard gates", () => {
  const build = readWorkflow("build.yml");

  it("runs version preflight before renderer and platform builds", () => {
    expect(build.jobs["release-preflight"]).toBeDefined();
    expect(build.jobs["renderer-box"].needs).toBe("release-preflight");
    const preflight = stepText(build.jobs["release-preflight"]);
    expect(preflight).toContain("release:preflight");
    expect(preflight).toContain("release-digest.v1.json");
    expect(preflight).toContain("release-digest.v2.json");
  });

  it("makes release depend on full quality plus Artifact bootstrap and historical-upgrade smoke", () => {
    expect(build.jobs["quality-gate"].needs).toBe("release-preflight");
    expect(build.jobs["quality-gate"].steps?.[0]?.with?.["fetch-depth"]).toBe(0);
    expect(build.jobs["artifact-release-smoke"].needs).toEqual(["build", "quality-gate"]);
    expect(stepText(build.jobs["artifact-release-smoke"])).toContain("test:artifact-release-smoke");
    expect(build.jobs.release.needs).toBe("artifact-release-smoke");

    const quality = stepText(build.jobs["quality-gate"]);
    expect(quality).toContain("npm run typecheck");
    expect(quality).toContain("npm run lint");
    expect(quality).toContain("npm run build:packages");
    expect(quality).toContain("npm test");
  });

  it("classifies prereleases with SemVer instead of publishing every tag as Latest", () => {
    const release = stepText(build.jobs.release);
    expect(release).toContain("semver.prerelease");
    expect(release).toContain("--prerelease=true");
    expect(release).toContain("--latest=false");
    expect(release).toContain("--prerelease=false");
    expect(release).toContain("--latest");
  });

  it("rechecks preflight before automatic Train pointer writes", () => {
    const publisher = build.jobs["publish-train"];
    expect(publisher.needs).toBe("release");
    const text = stepText(publisher);
    expect(text.indexOf("release:preflight")).toBeGreaterThanOrEqual(0);
    expect(text.indexOf("release:preflight")).toBeLessThan(text.indexOf("publish-train.mjs"));
  });
});

describe("manual Train workflow hard gates", () => {
  const manual = readWorkflow("publish-train.yml");
  const job = manual.jobs["publish-train"];

  it("checks out the requested tag with full history and preflights before materializing the signing key", () => {
    const steps = job.steps || [];
    const checkout = steps[0];
    expect(checkout?.with?.ref).toBe("${{ inputs.tag }}");
    expect(checkout?.with?.["fetch-depth"]).toBe(0);
    const preflightIndex = steps.findIndex((step) => String(step.run || "").includes("release:preflight"));
    const keyIndex = steps.findIndex((step) => step.name === "Materialize train signing key");
    const publishIndex = steps.findIndex((step) => String(step.run || "").includes("publish-train.mjs"));
    expect(preflightIndex).toBeGreaterThan(0);
    expect(keyIndex).toBeGreaterThan(preflightIndex);
    expect(publishIndex).toBeGreaterThan(keyIndex);
  });
});
