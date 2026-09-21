import path from "node:path";
import { fileURLToPath } from "node:url";
import { configDefaults, defineConfig } from "vitest/config";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  resolve: {
    alias: {
      // P01-T07：@hana/plugin-* 死别名清退（packages/ 已随插件生态收口拆除于
      // 04f90d2b2；清退前核实 tests/ 零引用）。@ 别名指向有效路径，保留。
      "@": path.resolve(__dirname, "desktop/src/react"),
    },
  },
  test: {
    exclude: [
      ...configDefaults.exclude,
      ".cache/**",
      // git worktree 副本有自己的测试快照，混进主树测试集会双份执行、断言错位
      ".claude/worktrees/**",
      "desktop/native/**/.build/**",
      "dist-computer-use/**",
    ],
    // CI 矩阵含 macos-15-intel 等慢 I/O runner：满载下单测可逼近旧 10s 默认
    // （64MB 缓冲分配/全仓扫描类），统一放宽到 60s；更慢的场景由各测试显式加预算。
    testTimeout: 60_000,
    hookTimeout: 60_000,
    setupFiles: ["./tests/setup-auto-updater.ts"],
    server: {
      deps: {
        inline: ["electron-updater", /desktop\/auto-updater/],
      },
    },
  },
});
