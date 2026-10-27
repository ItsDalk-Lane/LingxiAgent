# R10 延后测试台账

> 状态：`DEFERRED`（业主 2026-10-27 指示：R10 阶段中明确需要测试的项目全部延后，
> 本轮只做功能实现）。本文件不记录任何 PASS；未执行就是不执行。

## 1. R10 任务书任务/场景

| 项目 | 状态 | 说明 |
|---|---|---|
| R10-T01 核验全范围覆盖与完成定义 | DEFERRED | 入口 census/覆盖矩阵/需求差集均未执行 |
| R10-T02 完整链路安全与路径无关性 | DEFERRED | 静态/动态绕过矩阵未执行 |
| R10-T03 故障注入、取消与未知副作用 | DEFERRED | kill/断网/磁盘满/重连等未执行 |
| R10-T04 数据迁移、回滚与旧版本回归 | DEFERRED | 未用真实数据集演练 |
| R10-T05 真实供应商、平台与扩展验证 | DEFERRED | 未取得/未使用真实供应商与平台账号 |
| R10-T06 性能、稳定性与完整进程成本 | DEFERRED | 未采样性能/资源增长 |
| R10-T07 真实安装包、升级候选与签名 | DEFERRED | 未构建四平台安装包/未验证 updater/签名 |
| R10-T08 独立审查与最终缺口清零 | DEFERRED | 未做独立复核与最终门禁 |
| R10-A01～A16 | DEFERRED | 16 个验收场景全部未执行 |

## 2. 继承自 R09 的测试项

| 项目 | 状态 | 原因 |
|---|---|---|
| T04 麦克风/语音识别/通知 TCC 授权与撤销 | DEFERRED | 系统人工授权会话；OWNER-07 |
| T04 ASR 真实识别 | DEFERRED | 本地识别能力未实现；OWNER-05 |
| T03 通知点击聚焦人工项 | DEFERRED | 需真实点击/前台证据；本轮无 GUI 测试 |
| T05 Chromium PDF 运行时 | DEFERRED | 已知 CDP 抖动 + 发行物决策；本轮不改运行时 |
| T06 可见副作用 full 并集批次 | DEFERRED | 测试批次未执行 |
| T07 交付版文件任务自动点击 | DEFERRED | 交付 UI 无稳定文件任务入口，测试实现未完成 |
| T08 最终候选 Electron/Tauri 全量 A/B | DEFERRED | 测试项 |
| 三平台真机（macos-x64/windows-x64/linux-x64） | DEFERRED | OWNER-02：当前只做 macOS Apple 平台 |
| 正式签名/公证/安装器 | DEFERRED | OWNER-03：不做正式签名 |
| final-06 正式门 | DEFERRED | 测试项全部延后 |

## 3. 本轮明确不实现的功能（与测试延后分开登记）

| 项目 | 状态 | 原因 |
|---|---|---|
| 自动更新（auto_update_check/install/state/set_channel + event） | NOT_IMPLEMENTED | 发布通道、minisign 公钥、安装包产物未裁决；OWNER-03 不声称自动更新 |
| OTA 列车（train_update_status/check/apply + events） | NOT_IMPLEMENTED | 正式构建无 channel manifest/latest.json；后续需发布通道裁决 |
| Chromium 随包/分发 | DEFERRED | 决策：沿用系统 Chrome；见 `R10_CHROMIUM_DECISION.md` |
| 交付版文件任务入口/自动点击 | DEFERRED | 产品入口与验收方式未定；不伪造 PASS |

## 4. 本轮实现自检（不是 R10 验收）

编译、类型检查、构建对比与新增纯逻辑单测只用于防止实现破损，不作为 R10 通过依据：

- `cargo check` / `cargo test --lib`（desktop 宿主）
- `cargo check -p lingxi-service`
- `npx tsc --noEmit`
- 两次 `vite build` 的 `settings.html` 字节对比

未执行的 R10 验收测试不得用上述自检替代。
