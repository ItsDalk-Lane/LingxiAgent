# W11 FIFO 等待夹具修复与诊断（2026-10-14）

基线：`6d19332793c0819f5219a4ed42fcc8f0d7f813a4`。这是局部支持证据，R06 保持未验收。

## 确定事实与可复现机制

正式 driver 失败回执中的后续模型请求已包含工具错误：
`embedding journal temporary is not a regular file; paid bytes retained`。
该错误仅会在生产 `save_native` 的阻塞写打开已返回、实际句柄被确认不是普通文件后出现。
所以 `cancelVerdict=None`、`elapsedMs=0`、tasks 为空的直接原因是：测试预期仍阻塞的 FIFO 写打开提前返回，生产明确拒绝持久写 FIFO，工具/运行提前结束。
实际已知付费 usage 仍保留，故没有修改生产的账目或 FIFO 拒绝行为。

原夹具以 `0600` 创建 FIFO，任意同用户读者都能打开它。读打开会解除真实写打开等待；
因此「夹具只在测试显式 release 后放行」的前提没有被原权限保证。
在独立合成目录里，等付费 usage 落地后注入一个真实 reader，原 driver 测试确定失败：
exit 101、0.46 秒、`cancelVerdict=None`、`elapsedMs=0`、tasks `[]`，工具错误与正式门禁完全同文。
这证明一个可触发全部核心表征的夹具竞态；不是重跑后碰巧得到绿。

历史门禁实际 reader 的身份没有捕获。不能据此认定 Spotlight、杀毒、Codex 或门禁自身就是读者。
单独 Desktop 与 `/tmp` 裸 FIFO 探针均持续阻塞，lsof 未观察到 reader；也不能认定只要放在 Desktop 就一定会被打开。

## 已核查的替代因素

- 正式回执有具体的「非普通文件」拒绝，而非路径过长、产品资料缺失、端口占用或 transport 失败。
- `r06_stage_inputs.py` 的候选摘要枚举来自 git，排除 artifacts；没有读取本次 FIFO。
- xtask 的 run-output 归属脚本只做 fd/目录/文件类型归属，没有读取 FIFO 内容。
- `recover_embedding_usage_filtered` 的目录扫描只读取 `.json` 回执，`.pending` FIFO 不会被它作为回执读取。
- 压力/重复重放成功没有提供历史 reader 身份，不作为根因证据。

以上是已核查范围，不是对所有宿主读者的完全排除。

## 最小修复

仅修改 W11 测试文件，五处真实 FIFO 统一经 `create_native_fifo` 建立：
以 `0200` 创建，只允许 writer，并显式断言预释放阶段没有读权限。
仅 `release_native_fifo` 与失败清理先恢复 `0600`，随后打开 reader，释放真实 native 系统调用。
生产文件、门禁判定、已有取消/绝对期限/已付费账目/物理 lease/无晚写/容量断言均未改动，未增加 ignore。
其他同用户读者的提前 read 返回 EACCES，不能改变正在证明的等待交错。
新增一个同步用例，实际尝试 reader 并断言 PermissionDenied，确认真实 writer 持续等待，随后显式释放并 join。

## 验证

最终 W11 套件：原 11 项加新增夹具负控，**12 passed / 0 failed / 0 ignored**，25.62 秒。
`cargo fmt --all --check`、`git diff --check` 通过。

修后 reader 注入：实际 read open 返回 errno 13（EACCES），没有静默跳过；
原 driver 测试 exit 0，取消 verdict 为 StopUnconfirmed，真实 native task 在释放前仍为 running，
显式释放后退出。修前与修后紧凑回执见 [w11-validation.json](w11-validation.json)。
可复用探针见 [w11-reader-control.py](w11-reader-control.py)：对旧测试二进制用 `--expect allowed`，
对修后测试二进制用 `--expect denied`，输出必须指向全新目录。

原始本地日志保存在 `/tmp/lingxi-w11-diagnostic-20261014/`，不提交；紧凑回执绑定日志 SHA-256。
五个相关登记绑定由主控合并测试改动、重锚并重新生成图后执行，子代理没有修改登记或宣称它们已通过。
本机 macOS 的普通同用户 reader 隔离已验证；有绕过文件权限能力的特权读者、其他平台与正式阶段验收不在这份局部证明内。
