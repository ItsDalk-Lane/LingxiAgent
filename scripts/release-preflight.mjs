import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { pathToFileURL } from "node:url";
import semver from "semver";

import { ROOT, normalizeReleaseGeneration } from "./release-metadata.mjs";

function fail(message) {
  throw new Error(`release-preflight: ${message}`);
}

export function evaluateReleasePreflight({ candidateTag, candidateVersion, candidateGeneration, historicalReleases }) {
  if (candidateTag !== `v${candidateVersion}`) {
    fail(`tag ${candidateTag} does not match package version ${candidateVersion}`);
  }
  if (!semver.valid(candidateVersion)) {
    fail(`package version ${candidateVersion} is not valid SemVer`);
  }
  const generation = normalizeReleaseGeneration(candidateGeneration);
  const history = (historicalReleases || []).filter((entry) => entry.tag !== candidateTag);
  const validHistory = history.filter((entry) => semver.valid(entry.version));
  const maxVersion = validHistory.length === 0
    ? null
    : validHistory.map((entry) => entry.version).sort(semver.rcompare)[0];
  if (maxVersion && !semver.gt(candidateVersion, maxVersion)) {
    fail(
      `candidate ${candidateVersion} is not greater than historical maximum ${maxVersion}; `
        + "a released or prereleased installer may already be active on user devices",
    );
  }
  const maxGeneration = validHistory.reduce((max, entry) => {
    const value = Number.isInteger(entry.releaseGeneration) && entry.releaseGeneration > 0
      ? entry.releaseGeneration
      : 0;
    return Math.max(max, value);
  }, 0);
  if (generation <= maxGeneration) {
    fail(`releaseGeneration ${generation} must be greater than historical maximum ${maxGeneration}`);
  }
  return {
    candidateTag,
    candidateVersion,
    candidateGeneration: generation,
    historicalMaximumVersion: maxVersion,
    historicalMaximumGeneration: maxGeneration,
    historicalReleaseCount: validHistory.length,
  };
}

function git(args, cwd = ROOT) {
  return execFileSync("git", args, { cwd, encoding: "utf8" }).trim();
}

function readJsonAtRevision(revision, file, cwd = ROOT) {
  return JSON.parse(git(["show", `${revision}:${file}`], cwd));
}

export function collectProductReleases({ cwd = ROOT, candidateTag, packageName }) {
  const tags = git(["tag", "--list", "v*"], cwd)
    .split("\n")
    .map((value) => value.trim())
    .filter(Boolean);
  const releases = [];
  for (const tag of tags) {
    if (tag === candidateTag) continue;
    const tagVersion = tag.replace(/^v/, "");
    if (!semver.valid(tagVersion)) continue;
    let pkg;
    try {
      pkg = readJsonAtRevision(tag, "package.json", cwd);
    } catch {
      continue;
    }
    if (pkg.name !== packageName || pkg.version !== tagVersion) continue;
    releases.push({
      tag,
      version: pkg.version,
      releaseGeneration: pkg.lingxi && pkg.lingxi.releaseGeneration,
    });
  }
  return releases;
}

export function runReleasePreflight({ candidateTag, cwd = ROOT } = {}) {
  const pkg = JSON.parse(fs.readFileSync(path.join(cwd, "package.json"), "utf8"));
  const lock = JSON.parse(fs.readFileSync(path.join(cwd, "package-lock.json"), "utf8"));
  if (lock.version !== pkg.version || lock.packages?.[""]?.version !== pkg.version) {
    fail(
      `package-lock root versions (${lock.version}/${lock.packages?.[""]?.version}) do not match package version ${pkg.version}`,
    );
  }
  const tag = candidateTag || process.env.GITHUB_REF_NAME || `v${pkg.version}`;
  return evaluateReleasePreflight({
    candidateTag: tag,
    candidateVersion: pkg.version,
    candidateGeneration: pkg.lingxi && pkg.lingxi.releaseGeneration,
    historicalReleases: collectProductReleases({ cwd, candidateTag: tag, packageName: pkg.name }),
  });
}

function parseArgs(argv) {
  let candidateTag;
  for (let index = 0; index < argv.length; index += 1) {
    if (argv[index] === "--tag") candidateTag = argv[++index];
    else fail(`unknown argument ${argv[index]}`);
  }
  return { candidateTag };
}

if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  try {
    const result = runReleasePreflight(parseArgs(process.argv.slice(2)));
    console.log(`[release-preflight] PASS ${JSON.stringify(result)}`);
  } catch (error) {
    console.error(error?.stack || error?.message || String(error));
    process.exit(1);
  }
}
