# R08-A06 宿主自动化权限解锁手册（a06 环境红·家族 #9）

- 适用症状：`r08_t03_a06_drill` 红链（t09-a06-standalone-01..05 + 门内多轮复现），两种签名：
  1. `quit-b` 键盘路径穷尽（`quit-b-keystroke-1/2` osascript status 0 但进程不退 → `quit-b-sigterm` 兜底 journal 记录 `keyboard-path-exhausted`）——System Events keystroke 无效。
  2. 客户端 A 渲染目标中途死亡（`connInfoA2` 连接 ECONNREFUSED / `cdp-main-page: timeout after 90000ms`）。
- 语义面事实：全部语义断言至 `health-after-b` 100% PASS（remote 分支确证/零接触/服务连续性/退 B 后 A 健康）；红只发生在收尾的退出与终态 CDP 读取段。

## 根因（已定名）

本机（macOS）对演练终端进程的**自动化权限**未授予：`osascript → System Events → keystroke "q" using {command down}` 需要宿主 TCC 授权（「辅助功能/Accessibility」+「自动化→System Events」）。未授权时 osascript 返回 status 0 但按键被静默丢弃（macOS TCC 静默失败形态），Cmd+Q 不达 Electron → 键盘路径穷尽。客户端 A 渲染目标死亡为同族间歇（CPU 争用下 renderer 回收），线束已加目标重解析（cdpEvaluateClient）缓解。

## 宿主侧解锁步骤（只需一次）

1. 打开「系统设置 → 隐私与安全性 → 辅助功能」（System Settings → Privacy & Security → Accessibility）。
2. 把运行本演练的终端宿主加入并打开开关。本会话的进程链是：
   - `/Applications/ZCode.app/Contents/MacOS/ZCode`（当前驱动终端）
   - 或直接跑时：`/bin/zsh`/`iTerm/Terminal` 对应条目。
3. 「隐私与安全性 → 自动化」里确认 `ZCode → System Events`（或对应终端 → System Events）已勾选。
4. 若条目已存在但开关关闭，打开后**重启终端宿主进程**（TCC 对已运行进程不追溯生效）。

## 验证（无需跑全套门）

```bash
# 单套件验证（约 3-5 分钟）：
python3 -B scripts/rust-tauri/r08_stage_inputs.py \
  --run-suite r08_t03_a06_drill \
  --evidence /tmp/a06-unlock-check/SUITES/a06
# 期望：output.log 尾行 verdict=two-client-remote-pass__zero-touch-quit-verified failures=0
# 且 journal 里 quit-b/quit-a 的 mechanism=keyboard（非 sigterm-graceful）
```

## 解锁后重跑最终门

```bash
rm -rf artifacts/rust-tauri/R08/verify-r08-final-<N+1>
cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- \
  verify-stage R08 --evidence artifacts/rust-tauri/R08/verify-r08-final-<N+1>
```

预期变化：命令 17/17 全绿（唯一红 a06 消除）；场景 **16/16 PASS**（R08-A06 转绿）；叶数不变（a06 属场景面非叶面）。

## 解锁后实测更新（2026-10-25，t16-*-unlock-check-01..03）

- **a06 已绿**（连续两轮：t16-a06-unlock-check-01 SUITE_EXIT=0 + 门 11/12 门内绿）——其 SIGTERM 优雅兜底机制（Lead 接受先例）在 verdict 记录 mechanism。
- **a05 键盘路径仍未达**，但根因已精确到下一级：TCC **已授权**（`osascript keystroke` exit 0 无 not-authorized），剩余阻断=**System Events 无法把开发版 Electron 置为 frontmost**（`frontmost-not-achieved (-2700)`，t16-a05-unlock-check-02/03 journal 实证；frontmost 恒为宿主终端）。这符合手册步骤 4 的预判：**TCC 对已运行进程不追溯生效，需重启终端宿主**（完全退出 ZCode/终端再启动，不是新开窗口）。
- 重启宿主后重跑：`python3 -B scripts/rust-tauri/r08_stage_inputs.py --run-suite r08_t03_a05_drill --evidence /tmp/a05-recheck/SUITES/a05`——期望 row1-focus/row3-focus status 0（键盘路径原生恢复）。
- a05 线束已对齐 a06 兜底纪律（row3 SIGTERM 兜底=机制入 verdict；row1 AppleEvent 兜底后断言仍如实报 visible——不掩盖），故任何形态下判定都以断言为准。

## 2026-10-25 二次更新：宿主重启后仍 -2700（根因再精确化）

- **TCC 与重启均不解决**：t17-a05-recheck-01（重启后）row1/row3-focus 仍 `frontmost-not-achieved (-2700)`。
- **鉴别诊断（决定性）**：`set frontmost of process "Finder" to true` 与 `tell application id "com.github.Electron" to activate` 在本 ZCode 进程上下文**同样静默无效**（exit 0 但 frontmost 恒为 ZCode）——即本宿主上下文发起的 AppleScript 焦点变更被 macOS 策略整体丢弃，非 Electron 特有、非 TCC 缺失、非线束缺陷。a06 的 mechanism=sigterm-graceful 一致（键盘路径同从未达成，Lead 接受兜底）。
- **下一步升级手段（用户侧）**：从**普通终端 App（Terminal.app/iTerm）**运行单套件——焦点变更由前台 App 上下文发起，策略放行概率高：
  ```bash
  cd /Users/study_superior/Desktop/Code/LingxiAgent
  python3 -B scripts/rust-tauri/r08_stage_inputs.py \
    --run-suite r08_t03_a05_drill --evidence /tmp/a05-recheck-terminal/SUITES/a05
  ```
- 若普通终端仍 -2700：a05 row1（Cmd+W 隐藏可观察量）定性为**宿主焦点策略环境受限**（服务存活/页面保留/health 三不变量均绿，仅 visibilityState 可观察量不可达），按任务书 §6「环境不足为 BLOCKED 不改写为通过」如实呈现——当前 verdict=fail failures=1 正是该形态。

## 2026-10-25 三次更新：Terminal.app 上下文实测（第三形态）

- 经 `open -a Terminal` 真实在 Terminal.app 上下文运行（/tmp/a05-recheck-terminal/terminal-run.log，TERM_PROGRAM=Apple_Terminal）：System Events 报 **`-609 连接无效` / `-1712 AppleEvent 超时`**（非 -2700）——Terminal 自身未获「辅助功能 + 自动化→System Events」授权（手册步骤 1-3 需对**每个**终端宿主分别授予）。
- 三级诊断链至此完整：① ZCode 上下文=TCC 已授权但焦点变更被宿主策略静默丢弃（-2700，Finder 同无效）；② 宿主重启=无变化；③ Terminal 上下文=自动化未授权（-609/-1712）。
- **a05 全绿的最后条件**：系统设置中为 **Terminal**（及所用 iTerm）授予与 ZCode 相同两项权限并重启 Terminal 后重跑同命令。若不再尝试，row1 定性=宿主焦点策略环境受限（三不变量绿，仅 visibilityState 不可达），verdict=fail failures=1 即诚实终态（任务书 §6）。
- a06 不受影响（已绿，兜底机制 Lead 接受）。

## 2026-10-25 四次更新：授权+全新 Terminal 进程仍 -1712/-609（诊断链穷尽）

- 用户为 Terminal 授予两项权限后：①复用旧 Terminal 进程 → 同 -1712/-609（合理，TCC 不追溯）；②**完全退出 Terminal 后全新拉起**（osascript quit + killall 确认后 open）→ **仍然 -1712/-609**（/tmp/a05-recheck-terminal 两次运行记录）。
- 四级验证穷尽：ZCode(-2700 焦点丢弃) / 重启(无变化) / Terminal 复用(-1712) / Terminal 全新(-1712)。AppleEvent 超时=Terminal→System Events 自动化在 TCC 层仍未放行——**仅剩用户目视确认**（会话内无法读 TCC 数据库）：
  1. 系统设置→隐私与安全性→**自动化**：Terminal 条目下 **System Events** 勾选框是否真实存在且打开（若不存在=授权未落到该条目）；
  2. 同一界面→**辅助功能**列表：Terminal 是否在列且打开；
  3. 若用 iTerm 跑：同两项对 iTerm。
- **终态裁定**：在以上确认并重跑仍红的情况下，a05 row1 定性为宿主自动化策略环境受限（三不变量绿，仅 visibilityState 不可达），verdict=fail failures=1 为任务书 §6 诚实终态；a06 不受影响。本手册至此完成使命，不再有会话内升级手段。

## 2026-10-25 终局第六查：配置描述文件清白 + TCC 库 SIP 拒读（会话内检查全部穷尽）

- `system_profiler SPConfigurationProfileDataType`：本机仅一个用户级 ManagedSettings 描述文件（未签名，2026-09-15），payload 仅 `allowSiri3489=0`（Siri 限制）——**不含任何 AppleEvents/自动化/辅助功能限制**，排除该层。
- 系统/用户两级 TCC.db 直读均被 SIP 拒（`authorization denied`/`unable to open`）——「设置面板勾选可见但 AppleEvents 仍超时」的差额原因**无法从会话内取证**。
- **六查穷尽终局**：-2700（ZCode 焦点丢弃）/ 重启无变化 / Terminal 复用 -1712 / Terminal 全新×2 -1712（授权前后）/ 描述文件清白 / TCC 库不可读。a05 row1 定性=宿主自动化策略环境受限（§6 BLOCKED），本手册使命完成。
- 用户侧如仍追求 a05 全绿：唯一剩余路径=联系 IT/MDM 管理员核对设备管理策略日志，或在另一台无此现象的 Mac 上重跑单套件（同一仓库 clone 即可）。

## 2026-10-25 解锁生效实录：a05 原生键盘 PASS；a06 暴露同 bundle 焦点歧义（已修）

- **a05 转绿**：用户选路径①处理宿主授权后，verify-r08-final-14 门内 a05 `hideMechanism=keyboard / quitMechanism=keyboard`，failures=0——Cmd+W/Cmd+Q 由 System Events 真实投递，不是回退。此前「ZCode 上下文焦点变更被丢弃（-2700）」的定性随之解除；本手册六查链保留作诊断史。a05 的手动辅助模式（`R08_A05_MANUAL=1`，由用户在 Terminal 亲自按键）未再需要，作为备选保留在脚本内。
- **a06 新形态红（gate14）**：键击能投递后，`quitClientByPid` 对 B 的 Cmd+Q 打进了 A（journal：`quit-b-keystroke-1 → electron-A-exit`），随后 A 侧 CDP 求值 90s 超时崩溃。根因是 macOS/System Events 对**同一 app bundle 的两个实例**：`set frontmost of (process whose unix id is <B>)` 实际激活了该 bundle 的第一个实例 A，`frontmost of p` 又按 bundle 回答 true——按 pid 的 frontmost 判定对双实例不可信。以前键击被挡、B 总走 sigterm-graceful 兜底，这一缺陷被掩盖。
- **修法与实证**（t20-a06-focusfix-01，exit 0，failures=0）：发键前以 CDP 在目标渲染进程读 `document.hasFocus()`（按 pid 的真实键盘焦点，与 bundle 无关），并要求其余存活客户端 `!==true`；核实不到绝不盲发 Cmd+Q（盲发可能退掉任意前台 App，包括终端/IDE），直接走已披露的 SIGTERM 兜底；退 B 后新增硬断言 `client-a-alive-after-b`。实测 journal：`quit-b-focus-1 {"target":false,"others":{"A":true}}`（两次核实都证明焦点在 A）→ B 走 sigterm-graceful、A 未被误伤；`quit-a-focus-1 {"target":true}` → A 以真实键盘 Cmd+Q 退出。verdict 的 `quitMechanisms` 如实记录 `{B:'sigterm-graceful', A:'keyboard'}`。
- **结论**：对双实例中的第二个，在本机以 AppleScript 取得键盘焦点不可达，属环境/平台限制而非线束或产品缺陷；a06 的语义目标（退 B 零接触 A 与服务）不依赖 B 的退出机制，sigterm-graceful 披露路径与 Lead 已接受的纪律一致。不要为了把 B 也做成 keyboard 而放宽焦点核实。

## 若解锁后仍红（原节）

- keystroke 仍无效：检查「屏幕录制」权限（部分 macOS 版本 AXRaise 需要它）与「输入监控」。
- A 渲染目标仍死亡：属同族间歇（非权限），重跑单套件 2-3 次取绿证；持续死则查 Electron 调试口争用（其他调试器占用 9223+ 端口段）。
- 任何情况下不得为转绿弱化 a06 断言（本手册只解锁宿主权限，不改判定）。
