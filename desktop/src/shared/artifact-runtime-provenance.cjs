"use strict";

function component(componentState) {
  return {
    version: componentState.version || null,
    releaseGeneration: componentState.releaseGeneration ?? null,
    train: Number.isInteger(componentState.train) ? componentState.train : null,
    sha256: componentState.sha256 || null,
    slot: componentState.slot || null,
    source: componentState.source || "legacy",
    sourceCommit: componentState.sourceCommit || null,
    releasedAt: componentState.releasedAt || null,
  };
}

function buildArtifactRuntimeProvenance({ shellVersion, seed, server, renderer, compatibility }) {
  return {
    event: "artifact-runtime-provenance",
    shell: {
      version: shellVersion,
      releaseGeneration: seed.releaseGeneration ?? null,
      sourceCommit: seed.sourceCommit || null,
      releasedAt: seed.releasedAt || null,
    },
    server: component(server),
    renderer: component(renderer),
    compatibility,
  };
}

module.exports = { buildArtifactRuntimeProvenance };
