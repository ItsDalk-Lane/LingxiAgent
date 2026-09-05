import { spawn } from "node:child_process";
import { expect, it } from "vitest";

it("真实原生模块加载/建图期间重复关闭，子进程不崩溃且向量原值保留", async () => {
  const code = `
    import assert from 'node:assert/strict';
    import { annFixture } from './tests/helpers/knowledge-ann-fixture.ts';
    for (let iteration = 0; iteration < 30; iteration++) {
      const f = annFixture();
      try {
        f.add('closing', Array.from({length: 1300}, (_, index) => [1, index / 1300, 0]));
        const before = f.blobs(), backend = f.start();
        while (!backend.active?.worker) await new Promise(resolve => setImmediate(resolve));
        await new Promise(resolve => setTimeout(resolve, iteration % 12));
        await backend.close();
        assert.deepEqual(f.blobs(), before);
      } finally { await f.close(); }
    }
    console.log('30 native shutdowns verified');
  `;
  const result = await new Promise<{ code: number | null; signal: string | null; output: string }>((resolve, reject) => {
    const child = spawn(process.execPath, ["--input-type=module", "-e", code], { cwd: process.cwd(), stdio: ["ignore", "pipe", "pipe"] });
    let output = "";
    child.stdout.on("data", chunk => { output += chunk.toString(); });
    child.stderr.on("data", chunk => { output += chunk.toString(); });
    child.on("error", reject); child.on("close", (code, signal) => resolve({ code, signal, output }));
  });
  expect(result, result.output).toMatchObject({ code: 0, signal: null });
  expect(result.output).toContain("30 native shutdowns verified");
}, 30_000);
