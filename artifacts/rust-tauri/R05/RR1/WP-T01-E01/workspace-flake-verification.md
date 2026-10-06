# workspace-full.log 两项失败的单测复核（同一候选 lib，2026-10-04）

`cargo test --workspace --no-fail-fast`（workspace-full.log，exit 101）104 个测试二进制 ok，仅两项失败。两项在同一候选（本包全部改动已编译在内）上单独复跑均通过：

1. `r04_t08_tool_matrix::terminal_family_share_cases`
   - workspace 轮失败点：`terminal-snapshot-current-transcript: pinned expectation 1 did not hold (observed 0)` —— 纯 PTY 真实时序（cat tty 回显 + cursor tail 断言），无模型面/凭证参与（matrix_harness 不配置 model plane；属 R04 工具面）。
   - 复核：单测 `--test r04_t08_tool_matrix terminal_family_share_cases -- --test-threads=1` 通过；整套 `--test r04_t08_tool_matrix -- --test-threads=1` 10/10 通过（301.77s）。且同一 lib 的 service 全量轮（service-suite-after-fixtures.log）中该套件亦 10/10 通过。判定：并行负载下的 PTY 时序偶发，非本改动回归。
2. `r00_management_leaves::r00_management_positive_and_negative_branches_on_real_service`
   - 失败点为其自述的环境失败：`request to 192.168.3.5:… stalled … macOS application firewall / proxy TUN may be blocking inbound connections to this unsigned test binary`（web-auth/login，0 bytes）。无模型面参与。
   - 复核：单测 `--test r00_management_leaves -- --test-threads=1` 通过（156.65s）；service 全量轮亦通过（139.60s）。判定：macOS 防火墙对未签名测试二进制入站连接的间歇拦截，环境偶发，非本改动回归。

结论：工作区 104 ok + 2 环境偶发（均已单测复核通过）。无未解释的产品失败。
