"use strict";

/**
 * Artifact 的发布先后顺序。
 *
 * 产品版本负责面向用户展示；releaseGeneration 负责回答“哪一次发布更晚”。
 * 旧 manifest/指针没有 generation 时继续按 SemVer 比较，保证存量数据可读。
 */

const semver = require("semver");

function normalizeReleaseGeneration(value) {
  return Number.isInteger(value) && value > 0 ? value : null;
}

function looksLikeDifferentVersionScheme(leftVersion, rightVersion) {
  const left = semver.parse(leftVersion, { loose: false });
  const right = semver.parse(rightVersion, { loose: false });
  if (!left || !right || left.major !== right.major) return false;
  const minorDigits = (value) => (value <= 0 ? 1 : Math.floor(Math.log10(value)) + 1);
  return Math.abs(minorDigits(left.minor) - minorDigits(right.minor)) >= 1
    && (left.minor < 100) !== (right.minor < 100);
}

/**
 * @returns {-1|0|1|null}
 */
function compareProductVersions(leftVersion, rightVersion) {
  const left = semver.valid(leftVersion);
  const right = semver.valid(rightVersion);
  if (!left || !right) return null;
  return semver.compare(left, right);
}

/**
 * 新旧两侧都有 generation 时只看 generation；一侧进入新纪元后，新纪元永远
 * 胜过旧元数据。两侧都旧时退回产品 SemVer。无法安全比较时返回 null。
 * @param {{releaseGeneration?: number, version?: string}} candidate
 * @param {{releaseGeneration?: number, version?: string}} current
 * @returns {-1|0|1|null}
 */
function compareReleaseOrder(candidate, current) {
  const candidateGeneration = normalizeReleaseGeneration(candidate && candidate.releaseGeneration);
  const currentGeneration = normalizeReleaseGeneration(current && current.releaseGeneration);
  if (candidateGeneration !== null && currentGeneration !== null) {
    if (candidateGeneration === currentGeneration) return 0;
    return candidateGeneration > currentGeneration ? 1 : -1;
  }
  if (candidateGeneration !== null) return 1;
  if (currentGeneration !== null) return -1;
  return compareProductVersions(candidate && candidate.version, current && current.version);
}

/**
 * server 与 renderer 仍可独立回退；这里仅把危险的混合状态变成明确诊断。
 */
function assessRuntimeCompatibility(server, renderer) {
  const serverGeneration = normalizeReleaseGeneration(server && server.releaseGeneration);
  const rendererGeneration = normalizeReleaseGeneration(renderer && renderer.releaseGeneration);
  if (serverGeneration !== null && rendererGeneration !== null) {
    if (serverGeneration === rendererGeneration) {
      return { status: "compatible", reason: "same-release-generation" };
    }
    return { status: "warning", reason: "mixed-release-generation" };
  }
  if (serverGeneration !== null || rendererGeneration !== null) {
    return { status: "warning", reason: "mixed-generation-metadata" };
  }
  if (server && renderer && server.version === renderer.version) {
    return { status: "compatible", reason: "same-legacy-version" };
  }
  return { status: "warning", reason: "mixed-legacy-version" };
}

module.exports = {
  normalizeReleaseGeneration,
  looksLikeDifferentVersionScheme,
  compareProductVersions,
  compareReleaseOrder,
  assessRuntimeCompatibility,
};
