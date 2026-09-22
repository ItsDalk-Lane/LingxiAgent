import process from "node:process";
import fs from "node:fs";
// 相对本文件定位仓库源码（验收修复轮：原硬编码 /Users/... 绝对路径使
// tests/p02-recovery-restart.test.ts 只能在唯一机器路径下通过）。
import { TaskRegistry } from "../../../../lib/task-registry.ts";

const [persistencePath, sideEffectPath] = process.argv.slice(2);
const registry = new TaskRegistry({ persistencePath });
registry.registerHandler("p02-crash-probe", { abort: () => {} });
registry.register("task_probe_crash_1", { type: "p02-crash-probe" });
// 已承诺的外部动作（写替身实际计数）：结果落盘前进程被 kill。
fs.appendFileSync(sideEffectPath, `side-effect:${Date.now()}\n`, "utf8");
// 模拟崩溃：不调用 complete/fail，直接退出。
process.exit(70);
