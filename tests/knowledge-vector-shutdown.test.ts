import { spawn } from "node:child_process";
import { expect, it } from "vitest";

it("真实原生模块加载/建图期间重复关闭，子进程不崩溃且向量原值保留", async () => {
  const code = `
    import assert from 'node:assert/strict';
    import { annFixture } from './tests/helpers/knowledge-ann-fixture.ts';
    // 最多 30 轮、墙钟 45s 自限：本测试的目的是「建图期间反复关闭不崩溃且
    // 向量原值保留」，轮数是压力强度而非契约；慢速运行器满载时按时间提前
    // 收束，最少 8 轮仍构成真实的重复关闭压力。
    const deadline = Date.now() + 45_000;
    let completed = 0;
    while (completed < 30 && Date.now() < deadline) {
      const f = annFixture();
      try {
        f.add('closing', Array.from({length: 1300}, (_, index) => [1, index / 1300, 0]));
        const before = f.blobs(), backend = f.start();
        while (!backend.active?.worker) await new Promise(resolve => setImmediate(resolve));
        await new Promise(resolve => setTimeout(resolve, completed % 12));
        await backend.close();
        assert.deepEqual(f.blobs(), before);
      } finally { await f.close(); }
      completed += 1;
    }
    assert.ok(completed >= 8, 'expected at least 8 native shutdown iterations, got ' + completed);
    console.log(completed + ' native shutdowns verified');
  `;
  const result = await new Promise<{ code: number | null; signal: string | null; output: string }>((resolve, reject) => {
    const child = spawn(process.execPath, ["--input-type=module", "-e", code], { cwd: process.cwd(), stdio: ["ignore", "pipe", "pipe"] });
    let output = "";
    child.stdout.on("data", chunk => { output += chunk.toString(); });
    child.stderr.on("data", chunk => { output += chunk.toString(); });
    child.on("error", reject); child.on("close", (code, signal) => resolve({ code, signal, output }));
  });
  expect(result, result.output).toMatchObject({ code: 0, signal: null });
  expect(result.output).toMatch(/\d+ native shutdowns verified/);
  // 子进程串行执行 30 轮真实 usearch 建图/关闭；intel 慢速运行器在全量套件
  // 并行满载下曾 5 次超过 30s 预算（纯超时，断言从未失败）。给真实工作
  // 留足预算，不为压时间削轮数。
}, 120_000);
