# B-R2/R2 — F42 修复轮 1 证据（2026-10-06，修复智能体，全新上下文）

修复对象：`scripts/rust-tauri/r02_t08_legacy_entry_regression.sh`（F42 运行输出归属）。
`.gitignore` 未改动（修复选择 RR1 remaining 方案①：绑定侧归属，不动证据分发口径）。

## 前稿（首implementer 半成品）审计结论：保留架构、重写关键段

前稿的架构（sink 发现 + 多单元对称排除 + E0s fixtures）方向正确，予以保留；
但审查发现三处必须重写的缺陷，均未经其自检验证（其产物完成度未知、standalone
完整跑在 E5 分类循环中被终止，无最终 GREEN）：

1. **整个证据树可被排除（违反明令禁止）**：DIR 归属只查「artifacts 下且无
   tracked 内容」。sink 落在 `artifacts/x.log` → 排除单元 `artifacts`；
   声明根 `R02_A16_RUN_OUTPUT_ROOTS=artifacts` 通过其 case 判定（`artifacts|artifacts/*`）。
   修复：深度围栏（单元 ≥ artifacts/<area>/<stage>/…，bash case 模式
   `artifacts/*/*/*`，`*` 跨 `/` 匹配 = 深度下限 4）+ declared 专用校验函数。
2. **DIR 归属无 dedication 检查（旧静态证据可被掩蔽）**：sink 落在含前轮旧证据
   的目录会把整目录提升为排除单元。修复：单元内每个现存文件必须可归属本次运行
   （本身是 sink／在门禁证据子树内／在另一 sink 目录下），否则降级为 FILE 单元
   （仅排除 sink 文件本身）。
3. **发现调用点的重定向遮蔽 bug（本轮实测发现）**：`discover_run_output_sinks >
   $SINKS_FILE` 的调用点重定向会把门禁 bash 自身 fd-1 换成 SINKS_FILE，发现时刻
   看不见本次运行真实的 stdout sink（只有 stderr 经子进程可见）。s5-full 首跑即
   因此红（stdout.log 未归属）。修复：子 shell 调用 `( discover_run_output_sinks ) >
   $SINKS_FILE`，父进程真实 fd 全程可见。另把 lsof 字段显式化 `-Fpfn`（不依赖平台
   隐式输出 f 行）。

调试过程证据：debug-probe/（遮蔽 bug 现场：sink 文件只有 stderr.log 一行）、
debug-probe2/（修复后：DIR run-root）、debug-probe2/extracted-discovery-python.py、
s5-full/（首跑，因 bug 红在镜像 cmp）。

## 自检（全部真实执行；场景驱动脚本在 bin/，各自 WORK 归档在 <tag>/work/）

副本型场景 make_copy 跳过 rust/target（gitignored、不进绑定、门禁 npm 步骤不用；
等价性由 s1b 全量副本 + 前轮 standalone-full-1 真实全树交叉印证）。

| 场景 | 结果 |
|---|---|
| s1a 标准仓库内证据根 + stdout/stderr 落 artifacts 内 .log | PASS exit 0；DIR 证据目录 + DIR run-root 双单元 |
| s1b 仓库外证据根（/tmp）+ 仓库外捕获 | PASS exit 0；排除单元为空，绑定穷举（E0–E4.5 ALL GREEN） |
| s2 嵌套模拟（r04→r03→r02 三级，父子 heartbeat 同时增长） | PASS exit 0；四个 DIR 单元；三个 stdout.log 分别 72/72/97 行持续增长 |
| s3a 干净候选（副本内恢复 tracked+清 untracked，status=0） | PASS exit 0；"candidate worktree clean" |
| n1 绑定中途改真实源码 rust/.../lib.rs | RED：mirror cmp 失败 ✓ |
| n2 绑定中途新增源码文件 | RED ✓ |
| n3 绑定中途删除 tracked 脚本 | RED ✓ |
| n4a 绑定中途改旧静态 tracked 证据（R02/T01/a01-build.log） | RED ✓ |
| n4b 绑定中途改旧静态 untracked 证据（前轮遗留 .log，不在任何单元内） | RED ✓ |
| illegal R02_A16_RUN_OUTPUT_ROOTS=rust / artifacts / artifacts/rust-tauri | 三者全部绑定前拒绝（exit≠0，报错点名 declared run-output root）✓ |
| n6a sink 落在含旧证据的目录（dedication 降级） | PASS exit 0；仅 FILE 单元（run.log），旧证据保持绑定 |
| n6b 同形态 + 中途改旧证据 | RED：mirror cmp 失败 ✓ |
| E0s fixtures（每次门禁运行内建自检） | 全 PASS（含新增 declared 围栏 6 例 + FILE 单元 2 例 + 多单元 3 例） |
| bash -n | PASS |

## S5a 仓库根 standalone 完整跑

- s5-full/（首轮）：因上述调用点遮蔽 bug 红在镜像 cmp（bug 的在案证据）。
- s5-full-2/（修复后）：运行输出归属正确（DIR 证据目录 + DIR run-root，RR1 失败
  形态已消除），但镜像 cmp 红于**外部并发写入**：绑定 diff 证明窗口内 WP D 追加
  D-R2/workspace-test-*.log、新增 D-R2/r00-standalone-3.log，且 RR2_ISSUE_MATRIX.json
  / RR2_PROGRESS.md 被其他工作包更新——这是复制竞态检出的设计行为（fail-closed，
  保留无误），非 F42 缺陷。quiet 窗口重跑见 s5-full-3/（如存在）。

## 事故记录

recovery/incident-and-restore.txt：s3_clean.sh 首版清理管道的 rm 继承驱动 cwd，
误删真实树 1333 个 untracked-unignored 文件；用事发前 3 分钟的 s3a 快照副本按
「仅补缺失」规则全量恢复（still-missing=0）。该副本保留至收口。
