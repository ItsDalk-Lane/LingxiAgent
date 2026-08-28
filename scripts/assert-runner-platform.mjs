#!/usr/bin/env node

import path from "node:path";
import { fileURLToPath } from "node:url";

const SUPPORTED_RUNNERS = new Set([
  "darwin-arm64",
  "darwin-x64",
  "win32-x64",
  "linux-x64",
]);

export function assertRunnerPlatform({
  expectedPlatform,
  expectedArch,
  actualPlatform = process.platform,
  actualArch = process.arch,
}) {
  const expected = `${expectedPlatform}-${expectedArch}`;
  if (!SUPPORTED_RUNNERS.has(expected)) {
    throw new Error(`unsupported expected runner: ${expected}`);
  }
  if (actualPlatform !== expectedPlatform || actualArch !== expectedArch) {
    throw new Error(
      `runner mismatch: expected ${expected}, received ${actualPlatform}-${actualArch}`,
    );
  }
  return { platform: actualPlatform, arch: actualArch };
}

function main() {
  const expectedPlatform = process.argv[2];
  const expectedArch = process.argv[3];
  if (!expectedPlatform || !expectedArch) {
    throw new Error("Usage: node scripts/assert-runner-platform.mjs <platform> <arch>");
  }
  const actual = assertRunnerPlatform({ expectedPlatform, expectedArch });
  console.log(`runner platform verified: ${actual.platform}-${actual.arch}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}
