# R08 决策简报｜两条解锁路径，请您二选一

- 日期：2026-10-25（R08 会话已按任务书 §8 到达诚实关闭态）
- 终态实测（verify-r08-final-12）：叶 **14/126** PASS · 场景 **15/16** · 命令 **16/17** · overall=FAIL（诚实覆盖缺口表达）
- 两轮独立评审（T06 PASS-with-MINORS、R08 终审 ACCEPT_HONENT_CLOSURE）接受的即此关闭态

## 路径 ①：a05 全绿（最终门 → 17/17 命令 + 16/16 场景）

- **现状**：a05 唯一失败断言=row1-hide（Cmd+W 后 visibilityState=hidden）。六查穷尽：ZCode 上下文 TCC 已授权但焦点变更被宿主策略静默丢弃（-2700，Finder 同无效）/ 宿主重启无变化 / Terminal 复用与全新进程（授权前后）均 -1712/-609 / 配置描述文件清白（仅 allowSiri3489=0）/ 两级 TCC.db 直读被 SIP 拒。
- **您可做的两件事**：
  1. 联系 IT/MDM 管理员核对设备管理策略日志（设置面板层与描述文件层均已证明无限制，剩余差额只能从管理侧取证）；
  2. 或在另一台无此现象的 Mac 上 clone 同仓库重跑：
     ```bash
     python3 -B scripts/rust-tauri/r08_stage_inputs.py \
       --run-suite r08_t03_a05_drill --evidence /tmp/a05-recheck/SUITES/a05
     ```
- 成功后跑最终门（`cargo run --manifest-path rust/Cargo.toml -p xtask -- verify-stage R08 --evidence artifacts/rust-tauri/R08/verify-r08-final-<N+1>`）即达 17/17+16/16（叶数不变——a05 属场景面）。

## 路径 ②：显式授权开启 112 pending 叶补证新阶段（R09+）

- **范围**：D01 聊天/WS、D13 terminal WS、D20 CLI 三个域需**新线束设计**（本会话 A/B 线束形态不适用）；D02 余量约 53 叶多需产品侧合成前提（计划面板内容语义需模型工具合成、孤儿挂载、已删 Agent 会话等）；D-7/D-8/D-9/D-10 四起语义差与 PPG-1/PPG-3 需**产品裁决**（非测试问题）。
- **启动前置**：需要您显式授权（本简报即授权入口——回复授权即开工）；新线束设计文档先行（design-first 纪律同 R08）。
- **已就绪的地基**：A/B 线束（33 任务可扩展）、阶段图机器核对管线（full_original_behavior 案例唯一身份规则已趟通）、八轮补证方法论、六查诊断链模板。

## 一句话对照

| 路径 | 谁 | 达成 | 成本 |
|---|---|---|---|
| ① a05 解锁 | 您（IT/异机） | 17/17 命令 + 16/16 场景 | 低（单套件复验） |
| ② 112 叶补证 | 授权后新会话 | 逐步提升叶 PASS 数 | 高（新线束+产品裁决） |

两条路径互不阻塞，可先后或并行。
