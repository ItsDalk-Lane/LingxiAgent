import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

// 这里只锁定适配器安全边界；模型输出和性能由真实资产验收另行证明。
describe("原生本地模型适配器边界", () => {
  it("独立语音走标准管道，不监听网络，握手身份来自已校验运行包", () => {
    const source = fs.readFileSync(path.resolve("scripts/local-models/sherpa-sidecar.mjs"), "utf8");
    expect(source).toContain("input: process.stdin");
    expect(source).toContain("runtimeId: runtimeMetadata.id");
    expect(source).toContain("pid: process.pid");
    expect(source).not.toMatch(/node:(net|http|https)|fetch\(/);
    expect(source).toContain("await runtime?.dispose()");
  });
  it("旁路仅访问带鉴权的回环服务，并关闭在线下载和工具能力", () => {
    const source = fs.readFileSync(path.resolve("scripts/local-models/llama-sidecar.mjs"), "utf8");
    expect(source).toContain("'--host', '127.0.0.1'");
    expect(source).toContain("'--offline'");
    expect(source).toContain("'--no-agent'");
    expect(source).toContain("'--flash-attn', 'auto'");
    expect(source).toContain("'--cache-ram', '0'");
    expect(source).toContain("LLAMA_API_KEY: token");
    expect(source).toContain("redirect: 'error'");
    expect(source).toContain("if (!ready) process.stderr.write");
  });

  it("语音取消采用原生协作收尾，禁止在推理期间强杀工作线程", () => {
    const runtime = fs.readFileSync(path.resolve("scripts/local-models/sherpa-runtime.mjs"), "utf8");
    const worker = fs.readFileSync(path.resolve("scripts/local-models/sherpa-worker.mjs"), "utf8");
    expect(runtime).not.toContain(".terminate(");
    expect(runtime).toContain("Atomics.store(cancellation, 0, 1)");
    expect(runtime).toContain("await queue; await stop()");
    expect(worker).toContain("await model.generateAsync");
    expect(worker).toContain("onProgress: () => Atomics.load(cancellation, 0) === 0");
    expect(worker).toContain("parentPort.close()");
  });
});
