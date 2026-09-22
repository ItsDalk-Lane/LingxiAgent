// P00-A05「失败诚实」：一个必然失败的独立测试用例。
// 命名避开 vitest 默认 include（**/*.{test,spec}.*），只能显式以
//   node --test artifacts/refactor-2026/P00/fixtures/fail-demo/deliberate-fail.case.mjs
// 运行；它不属于 npm test 基线集合，退出码如实为 1。
import test from "node:test";
import process from "node:process";
import assert from "node:assert/strict";

test("P00-A05 deliberate failure: 2 + 2 must equal 5 (intentionally wrong expectation)", () => {
  // 该断言设计为失败：用于证明失败被如实记录为 FAIL，而不是汇总成 PASS。
  assert.equal(2 + 2, 5);
});
