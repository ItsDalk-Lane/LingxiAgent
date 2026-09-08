/**
 * A13–A14：speech helper 与原生授权桥的构建/打包合同。
 *
 * 合同级断言（脚本接线、架构映射、产物落点）在这里自动化；真实 swift 编译、
 * Electron 加载 .node、签名链验证属于真机验收（A14 真机部分另记）。
 */
import fs from "fs";
import os from "os";
import path from "path";
import { fileURLToPath } from "url";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  buildSpeechHelper,
  swiftArchForNodeArch,
} from "../scripts/build-speech-helper.mjs";
import {
  buildSpeechPermissionsBridge,
  SPEECH_PERMISSIONS_ARTIFACT_NAME,
} from "../scripts/build-speech-permissions.mjs";

const packageJsonPath = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../package.json");
const packageJson = JSON.parse(fs.readFileSync(packageJsonPath, "utf-8"));

let tmpDir: string;

beforeEach(() => {
  tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-speech-build-"));
});

afterEach(() => {
  fs.rmSync(tmpDir, { recursive: true, force: true });
});

describe("speech helper build contract（A13–A14）", () => {
  it("A13：pack / dist 入口真实接线 build:speech-helper 与授权桥构建，且先于 electron-builder", () => {
    for (const name of ["pack", "dist"]) {
      const script = packageJson.scripts[name];
      expect(typeof script).toBe("string");
      expect(script).toContain("build:speech-helper");
      expect(script).toContain("build:speech-permissions");
      // 构建必须先于打包：electron-builder 出现在所有构建步骤之后。
      expect(script.indexOf("build:speech-helper")).toBeLessThan(script.indexOf("electron-builder"));
      expect(script.indexOf("build:speech-permissions")).toBeLessThan(script.indexOf("electron-builder"));
    }
    // extraResources 把 dist-speech 产物目录带进 Resources/speech/macos/。
    // 语音 helper 是 macOS 专属：映射允许挂在顶层或 build.mac 级（mac 级才是
    // 正确落点——不往 Windows/Linux 安装包里塞 mac 二进制）。
    const extraResources = [
      ...(packageJson.build?.extraResources ?? []),
      ...(packageJson.build?.mac?.extraResources ?? []),
    ];
    const speechMapping = extraResources.find((entry: any) =>
      String(entry?.from ?? "").includes("dist-speech/mac-")
      && String(entry?.to ?? "").includes("speech/macos"));
    expect(speechMapping).toBeTruthy();
  });

  it("A13b：干净 checkout（无 dist-speech）下构建链生成所需产物", () => {
    const swiftBuildArgs: string[][] = [];
    const fakeBin = path.join(tmpDir, "bin");
    fs.mkdirSync(fakeBin, { recursive: true });
    const fakeProduct = path.join(fakeBin, "lingxi-speech-helper");
    const result = buildSpeechHelper({
      platform: "darwin",
      arch: "arm64",
      rootDir: tmpDir,
      run: (_cmd, args) => {
        swiftBuildArgs.push([...args]);
        // 模拟 swift build 产出二进制。
        fs.writeFileSync(fakeProduct, "fake-macho");
      },
      read: () => fakeBin,
    });
    expect(result.skipped).toBe(false);
    const expected = path.join(tmpDir, "dist-speech", "mac-arm64", "lingxi-speech-helper");
    expect(fs.existsSync(expected)).toBe(true);
    // 可执行权限随产物落盘。
    expect(fs.statSync(expected).mode & 0o111).not.toBe(0);
    expect(swiftBuildArgs[0]).toContain("--arch");
  });

  it("A14：架构映射与产物架构一致（arm64→arm64，x64→x86_64），不支持架构明确报错", () => {
    expect(swiftArchForNodeArch("arm64")).toBe("arm64");
    expect(swiftArchForNodeArch("x64")).toBe("x86_64");
    expect(() => swiftArchForNodeArch("ia32")).toThrow();

    const seenArch: string[] = [];
    const fakeBin = path.join(tmpDir, "bin-x64");
    fs.mkdirSync(fakeBin, { recursive: true });
    fs.writeFileSync(path.join(fakeBin, "lingxi-speech-helper"), "fake-macho-x64");
    buildSpeechHelper({
      platform: "darwin",
      arch: "x64",
      rootDir: tmpDir,
      run: (_cmd, args) => {
        const index = args.indexOf("--arch");
        seenArch.push(args[index + 1]);
      },
      read: () => fakeBin,
    });
    expect(seenArch).toEqual(["x86_64"]);
    expect(fs.existsSync(path.join(tmpDir, "dist-speech", "mac-x64", "lingxi-speech-helper"))).toBe(true);
  });

  it("A14b：授权桥以 Electron 头文件构建（node-gyp --target/--dist-url），产物随架构落盘", () => {
    const gypArgs: string[][] = [];
    const fakeBuildDir = path.join(tmpDir, "bridge-build");
    const fakeProduct = path.join(fakeBuildDir, "Release", SPEECH_PERMISSIONS_ARTIFACT_NAME);
    const result = buildSpeechPermissionsBridge({
      platform: "darwin",
      arch: "arm64",
      rootDir: tmpDir,
      electronVersion: "42.8.1",
      buildDir: fakeBuildDir,
      run: (_cmd, args) => {
        gypArgs.push([...args]);
        fs.mkdirSync(path.dirname(fakeProduct), { recursive: true });
        fs.writeFileSync(fakeProduct, "fake-node");
      },
    });
    expect(result.skipped).toBe(false);
    const flat = gypArgs.flat().join(" ");
    // Electron ABI 头文件是硬合同：不得以本机 Node 头文件冒充。
    expect(flat).toContain("--target=42.8.1");
    expect(flat).toMatch(/--dist-url=\S*(electron|headers)\S*/i);
    expect(result.target).toBe(path.join(tmpDir, "dist-speech", "mac-arm64", SPEECH_PERMISSIONS_ARTIFACT_NAME));
    expect(fs.existsSync(result.target!)).toBe(true);
  });

  it("非 macOS 两个构建都跳过", () => {
    const helper = buildSpeechHelper({ platform: "linux", rootDir: tmpDir, run: () => {}, read: () => "" });
    expect(helper.skipped).toBe(true);
    const bridge = buildSpeechPermissionsBridge({ platform: "win32", rootDir: tmpDir, run: () => {} });
    expect(bridge.skipped).toBe(true);
  });
});
