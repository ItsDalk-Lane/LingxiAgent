import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

export function normalizeSourceCommit(value) {
  const commit = String(value || "").trim().toLowerCase();
  if (!/^[0-9a-f]{40,64}$/.test(commit)) {
    throw new Error(`release metadata: sourceCommit must be a 40-64 character hex commit id, got ${JSON.stringify(value)}`);
  }
  return commit;
}

export function normalizeReleaseGeneration(value) {
  if (!Number.isInteger(value) || value <= 0) {
    throw new Error(`release metadata: releaseGeneration must be a positive integer, got ${JSON.stringify(value)}`);
  }
  return value;
}

export function resolveSourceCommit(env = process.env, exec = execFileSync, cwd = ROOT) {
  const explicit = env.LINGXI_SOURCE_COMMIT || env.GITHUB_SHA;
  if (explicit) return normalizeSourceCommit(explicit);
  return normalizeSourceCommit(exec("git", ["rev-parse", "HEAD"], { cwd, encoding: "utf8" }));
}

export function readReleaseMetadata(packagePath = path.join(ROOT, "package.json"), opts = {}) {
  const pkg = JSON.parse(fs.readFileSync(packagePath, "utf8"));
  const sourceCommit = opts.sourceCommit
    ? normalizeSourceCommit(opts.sourceCommit)
    : resolveSourceCommit(opts.env, opts.exec, opts.cwd || path.dirname(packagePath));
  return {
    version: pkg.version,
    releaseGeneration: normalizeReleaseGeneration(pkg.lingxi && pkg.lingxi.releaseGeneration),
    sourceCommit,
  };
}

export { ROOT };
