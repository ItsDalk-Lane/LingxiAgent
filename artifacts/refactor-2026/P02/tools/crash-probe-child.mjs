import process from "node:process";
import { TaskRegistry } from "/Users/study_superior/Desktop/Code/LingxiAgent/lib/task-registry.ts";
import fs from "node:fs";

const [persistencePath, sideEffectPath] = process.argv.slice(2);
const registry = new TaskRegistry({ persistencePath });
registry.registerHandler("p02-crash-probe", { abort: () => {} });
registry.register("task_probe_crash_1", { type: "p02-crash-probe" });
// 已承诺的外部动作（写替身实际计数）：结果落盘前进程被 kill。
fs.appendFileSync(sideEffectPath, `side-effect:${Date.now()}\n`, "utf8");
// 模拟崩溃：不调用 complete/fail，直接退出。
process.exit(70);
