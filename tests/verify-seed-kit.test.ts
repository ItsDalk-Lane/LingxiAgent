import { generateKeyPairSync, sign as cryptoSign } from "crypto";
import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, describe, expect, it } from "vitest";

import { packDualKindSeed, seedManifestFileName } from "../scripts/build-server-artifact.mjs";
import { verifySeedKit } from "../scripts/verify-seed-kit.mjs";

const tempDirs: string[] = [];
const OFFICE_FONT_FIXTURE_CSS = `
@font-face { font-family: 'EB Garamond'; src: url('./fonts/eb.woff2') format('woff2'); }
@font-face { font-family: 'Noto Serif SC'; src: url('./fonts/noto.woff2') format('woff2'); }
@font-face { font-family: 'JetBrains Mono'; src: url('./fonts/mono.woff2') format('woff2'); }
`;

function makeTempDir(prefix: string) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  tempDirs.push(dir);
  return dir;
}

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

function makeSigningIdentity(root: string, keyId = "verify-seed-kit-test") {
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  const privatePem = privateKey.export({ type: "pkcs8", format: "pem" }).toString();
  const publicPem = publicKey.export({ type: "spki", format: "pem" }).toString();
  const keyPath = path.join(root, "test-sign-key.pem");
  const keysetPath = path.join(root, "test-keyset.json");
  fs.writeFileSync(keyPath, privatePem, { mode: 0o600 });
  const keyset = [{ keyId, publicKey: publicPem }];
  fs.writeFileSync(keysetPath, JSON.stringify(keyset, null, 2));
  return { keyId, keyPath, keysetPath, keyset, privateKey };
}

function makeServerTree(root: string, marker = "server") {
  const outDir = path.join(root, "dist-server", marker);
  fs.mkdirSync(path.join(outDir, "bundle"), { recursive: true });
  fs.writeFileSync(path.join(outDir, "bundle", "index.js"), `console.log(${JSON.stringify(marker)});\n`);
  fs.writeFileSync(path.join(outDir, "hana-server"), "#!/bin/sh\nexit 0\n", { mode: 0o755 });
  return outDir;
}

function makeRendererTree(root: string) {
  const rendererDir = path.join(root, "dist-renderer");
  fs.mkdirSync(path.join(rendererDir, "assets"), { recursive: true });
  fs.writeFileSync(path.join(rendererDir, "index.html"), "<!doctype html><html></html>\n");
  fs.writeFileSync(path.join(rendererDir, "assets", "index.js"), "console.log('renderer');\n");
  const themesDir = path.join(rendererDir, "themes");
  const fontsDir = path.join(themesDir, "fonts");
  fs.mkdirSync(fontsDir, { recursive: true });
  fs.writeFileSync(path.join(themesDir, "new-warm-paper-fonts.css"), OFFICE_FONT_FIXTURE_CSS, "utf8");
  for (const fileName of ["eb.woff2", "noto.woff2", "mono.woff2"]) {
    fs.writeFileSync(path.join(fontsDir, fileName), Buffer.from("wOF2fixture"));
  }
  return rendererDir;
}

/** Builds a genuine, correctly-signed seed kit via the real packDualKindSeed
 * pipeline — the fixture is production code exercising itself, not a
 * hand-rolled manifest object that could drift from the real shape. */
async function buildValidKit(root: string, opts: { platform?: string; arch?: string } = {}) {
  const platform = opts.platform ?? "linux";
  const arch = opts.arch ?? "x64";
  const identity = makeSigningIdentity(root);
  const artifactOutDir = path.join(root, "dist-server-artifact", `${platform}-${arch}`);
  await packDualKindSeed({
    outDir: makeServerTree(root, `${platform}-${arch}`),
    rendererDistDir: makeRendererTree(root),
    rendererArtifactOutDir: path.join(root, "dist-renderer-artifact"),
    artifactOutDir,
    version: "0.381.0",
    platform,
    arch,
    env: { HANA_SIGN_KEY: identity.keyPath, HANA_SIGN_KEYSET: identity.keysetPath },
    log: () => {},
  });
  return { artifactOutDir, platformArch: `${platform}-${arch}`, identity };
}

/** Rewrites the manifest with `mutate` applied and re-signs it with the same
 * key — isolates "manifest content is wrong" from "signature is wrong" so
 * each negative case only trips the ONE check it targets. */
function tamperManifestField(
  artifactOutDir: string,
  platformArch: string,
  privateKey: ReturnType<typeof generateKeyPairSync>["privateKey"],
  mutate: (manifest: any) => void,
) {
  const manifestPath = path.join(artifactOutDir, seedManifestFileName(platformArch));
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  mutate(manifest);
  const bytes = Buffer.from(JSON.stringify(manifest, null, 2) + "\n", "utf8");
  fs.writeFileSync(manifestPath, bytes);
  fs.writeFileSync(`${manifestPath}.sig`, cryptoSign(null, bytes, privateKey));
}

describe("verify-seed-kit: verifySeedKit (positive case)", () => {
  it("passes for a freshly packed, correctly signed seed kit", async () => {
    const root = makeTempDir("hana-verify-seed-kit-ok-");
    const { artifactOutDir, platformArch, identity } = await buildValidKit(root);

    const result = await verifySeedKit({ artifactOutDir, platformArch, keyset: identity.keyset });

    expect(result).toEqual({ ok: true, errors: [] });
  });
});

describe("verify-seed-kit: verifySeedKit (negative cases)", () => {
  it("fails when the manifest signature is tampered", async () => {
    const root = makeTempDir("hana-verify-seed-kit-sig-");
    const { artifactOutDir, platformArch, identity } = await buildValidKit(root);
    const sigPath = `${path.join(artifactOutDir, seedManifestFileName(platformArch))}.sig`;
    const sig = fs.readFileSync(sigPath);
    sig[0] ^= 0xff;
    fs.writeFileSync(sigPath, sig);

    const result = await verifySeedKit({ artifactOutDir, platformArch, keyset: identity.keyset });

    expect(result.ok).toBe(false);
    expect(result.errors.some((e) => /signature verification failed/i.test(e))).toBe(true);
  });

  it("fails when the manifest's platform key does not match the directory it was found in", async () => {
    const root = makeTempDir("hana-verify-seed-kit-platform-");
    // Build a real linux-x64 kit, then copy it byte-for-byte into a
    // win32-x64-named directory/manifest — the exact class of mistake the
    // per-platform rename is meant to catch: a manifest whose filename
    // claims one platform-arch while its SIGNED CONTENT (artifacts.server
    // key) still says another. Bytes are untouched, so the signature check
    // must still PASS — this isolates "wrong platform key" as its own,
    // independent failure.
    const { artifactOutDir: linuxDir, identity } = await buildValidKit(root, { platform: "linux", arch: "x64" });
    const winDir = path.join(root, "dist-server-artifact", "win32-x64");
    fs.mkdirSync(winDir, { recursive: true });
    for (const name of fs.readdirSync(linuxDir)) {
      fs.copyFileSync(path.join(linuxDir, name), path.join(winDir, name));
    }
    const linuxManifestName = seedManifestFileName("linux-x64");
    const winManifestName = seedManifestFileName("win32-x64");
    fs.renameSync(path.join(winDir, linuxManifestName), path.join(winDir, winManifestName));
    fs.renameSync(path.join(winDir, `${linuxManifestName}.sig`), path.join(winDir, `${winManifestName}.sig`));

    const result = await verifySeedKit({ artifactOutDir: winDir, platformArch: "win32-x64", keyset: identity.keyset });

    expect(result.ok).toBe(false);
    expect(result.errors.some((e) => e.includes('artifacts.server["win32-x64"]'))).toBe(true);
    expect(result.errors.some((e) => /signature verification failed/i.test(e))).toBe(false);
  });

  it("fails when the manifest's recorded sha256 does not match the actual archive bytes", async () => {
    const root = makeTempDir("hana-verify-seed-kit-sha-");
    const { artifactOutDir, platformArch, identity } = await buildValidKit(root);
    tamperManifestField(artifactOutDir, platformArch, identity.privateKey, (manifest) => {
      manifest.artifacts.server[platformArch].sha256 = "0".repeat(64);
    });

    const result = await verifySeedKit({ artifactOutDir, platformArch, keyset: identity.keyset });

    expect(result.ok).toBe(false);
    expect(result.errors.some((e) => e.includes("sha256 mismatch"))).toBe(true);
    expect(result.errors.some((e) => e.includes("size mismatch"))).toBe(false);
    expect(result.errors.some((e) => /signature verification failed/i.test(e))).toBe(false);
  });

  it("fails when the manifest's recorded size does not match the actual archive size", async () => {
    const root = makeTempDir("hana-verify-seed-kit-size-");
    const { artifactOutDir, platformArch, identity } = await buildValidKit(root);
    tamperManifestField(artifactOutDir, platformArch, identity.privateKey, (manifest) => {
      manifest.artifacts.renderer.size += 1;
    });

    const result = await verifySeedKit({ artifactOutDir, platformArch, keyset: identity.keyset });

    expect(result.ok).toBe(false);
    expect(result.errors.some((e) => e.includes("size mismatch"))).toBe(true);
    expect(result.errors.some((e) => e.includes("sha256 mismatch"))).toBe(false);
    expect(result.errors.some((e) => /signature verification failed/i.test(e))).toBe(false);
  });

  it("fails when an archive the manifest references is missing entirely", async () => {
    const root = makeTempDir("hana-verify-seed-kit-missing-archive-");
    const { artifactOutDir, platformArch, identity } = await buildValidKit(root);
    const manifest = JSON.parse(fs.readFileSync(path.join(artifactOutDir, seedManifestFileName(platformArch)), "utf8"));
    fs.rmSync(path.join(artifactOutDir, manifest.artifacts.renderer.path));

    const result = await verifySeedKit({ artifactOutDir, platformArch, keyset: identity.keyset });

    expect(result.ok).toBe(false);
    expect(result.errors.some((e) => e.includes("archive referenced by manifest is missing"))).toBe(true);
  });

  it("fails when the manifest file itself is missing", async () => {
    const root = makeTempDir("hana-verify-seed-kit-missing-manifest-");
    const { artifactOutDir, platformArch, identity } = await buildValidKit(root);
    fs.rmSync(path.join(artifactOutDir, seedManifestFileName(platformArch)));

    const result = await verifySeedKit({ artifactOutDir, platformArch, keyset: identity.keyset });

    expect(result.ok).toBe(false);
    expect(result.errors.some((e) => e.startsWith("manifest missing"))).toBe(true);
  });
});
