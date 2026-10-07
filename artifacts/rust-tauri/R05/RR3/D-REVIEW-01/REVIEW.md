# RR3 D-REVIEW-01 独立环境审查

结论：**D定位/操作准备独立核验 PASS；required r00 gate = FAIL；必需环境项 = BLOCKED。** 环境没有解除，不签 A/C/F46 或阶段 PASS，R06_READY 仍为 false。

审查者 rr3_d_review_01，真实入口是已安装 Codex CLI `exec --ephemeral`、空历史；thread_id=`01a113ec-7810-78c3-8cb6-a43f806b00a9`。dispatch/request.json 和事件首行为入口依据，未声称 collaboration spawn 成功，未参与旧D或其他RR3实施/判断。本审查未派代理、未外发消息、未请求用户权限。dispatch 由父级持有，本审查不改写。继承权威和读取索引见 inherited-reading-index.json；当前大表按D完整相关记录读取，不把历史阶段结论当现状。

## 对象、来源与历史差异

HEAD/branch：`b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b` / `codex/rust-tauri-migration`，加现有未提交 RR3 修复与文档。实际完整 git status/stat 在 identity-before/after.json；没有宣称干净树或全部改动已提交。

macOS27.0.1(26A434)、arm64；绝对 `/Users/study_superior/.cargo/bin/cargo +1.98.1` 与 rustc1.98.1，Node24.16.0/npm11.13.0。Cargo工具路径自然链接到rustup代理，该代理不是被测产物。构建环境（包括SDKROOT）、未设置的RUSTFLAGS/target/profile变量、Cargo配置位置和工具真实退出在 build-environment.json；没有改环境来挑绿。

本轮新预构建 `--locked --no-run --message-format=json` exit0，98.341s，CargoJSON唯一匹配 package=lingxi-service、target.name=r00_management_leaves、kind=test、profile.test=true；fresh=false，新重链接。原始CargoJSON完整保存，不靠同名猜身份。

产物绝对路径：

`/Users/study_superior/Desktop/Code/LingxiAgent/rust/target/debug/deps/r00_management_leaves-294787929158efb0`

- SHA256=`9f7489029c91d1c232e3c204236854bd53c69cee2fef33d6fd778a27f44696d3`
- CDHash=`d9676388f524872f1269c13f61f413458c0e9e52`
- CDHashFull=`d9676388f524872f1269c13f61f413458c0e9e5246adbc3b7ccb9da04054687d`
- 非符号链接 Mach-O arm64，linker ad-hoc签名，TeamIdentifier未设置；完整签名与codesign验证exit0保留。前/运行中/后文件SHA、CDHash一致。
- 预构建前/运行前/运行后355个Rust/脚本/锁/工具链输入manifest完全一致，digest=`0f945b9884ee73957a6db3fe19c3d6b4a6b87672feef7c3fd1b46998f2f92b75`。不包含增长中的证据/现行文档，不将此摘要冒充整个工作区全局绑定；真实脏树另存。
- r00测试源码SHA=`3e72c2a2d16287549a2727a355642ec279c09595d42d6ec42d9c5585c8d2f453`，与HEAD及D-01逐字节相等；原20秒交换、真实if_addrs非回环选择、0.0.0.0:0监听与LAN Origin/登录/会话/注销断言都保留，零ignore/零人为filter。
- Cargo.lock SHA=`259f983e98a4da13eab1b592c2efd7b79579ad6678a08995a4b9e3fca618eff3`；工具链/manifest/logging/fixture源码及生成schema逐文件摘要在 schema-config-fixture-inputs.json。

D-01 manifest28/28大小和SHA全部一致，完整raw程序化读验（historical-D-audit.json）。历史r00自然exit101、0/1/0/0及旧失败全部保留。旧SHA c5975a45…/CDHash6eadd46c…生成于F46前；本轮同路径已换身份，旧PREPARED不可沿用。与D-01结束输入相比，logging.rs、资源测试、A永久回归脚本及两shell所在脚本共4个输入变化，详见D01-to-current-input-difference.json；没有抹掉正确的历史绿窗，也没有继承它为本轮通过。

## 本人亲跑：一次真实原套件

完整argv、UTC、真实exit、计数、日志SHA与监督信号见 commands.json / r00-formal-01.json。cwd为仓库根。

```sh
/Users/study_superior/.cargo/bin/cargo +1.98.1 test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r00_management_leaves -- --nocapture
```

2026-10-07 01:18:32.963893Z → 01:19:36.114671Z（本地09:18:32→09:19:36）；总63.150s，test自身52.75s。**自然exit101，0 passed/1 failed/0 ignored/0 measured/0 filtered**，唯一测试 `r00_management_positive_and_negative_branches_on_real_service` FAILED。监督上限180s未触发，supervisorTimedOut=false，TERM/KILL列表为空；没有重试或挑绿。

真实cargo PID86486，测试PID86488，运行中再次只读核身份与CargoJSON对象相等。监听初期127.0.0.1:60152，后为 `TCP *:60281 (LISTEN)`；源码绑定0.0.0.0。真实请求到192.168.3.5:60281、POST /lingxi/v1/web-auth/login，原write/read exchange20s内读取0字节，panic于原失败出口。由源码顺序可知停在LAN foreign-Origin首次请求，没有完成其403或后续LAN正向断言，不把回环管理分支成功当整个test通过。

监听原始ps/lsof及机器字段、每条读取exit/UTC保存在 r00-formal-01-listeners.jsonl。计划间隔1s；首次运行中完整签名验证造成约13s采样间隔，此后按约1s取样，未称连续无间隔观测。结束后被测PID和探针已不在只读进程表；未杀任何别人的进程。失败测试未到末尾合成home清理段，可能保留本次合成临时目录，本审查未删除它或用户数据。

## 本人最小真实差分

本轮自写零Lingxi C与Python探针，仅写本证据目录。C先bind/listen，再nonblocking connect/write，然后poll→accept/recv→响应；Python同顺序。等待共享六秒截止，外层15s；没有sleep猜就绪。编译exit0，未手动重签。探测前/后C、Apple shim和实际解释器都核SHA/CDHash/完整签名/验证，身份一致。

| 入口 | 真实地址/随机端口 | 实际结果 |
|---|---|---|
| 自有C同二进制回环 | 0.0.0.0:60177 →127.0.0.1:60177 | exit0，accepted=1，PING/PONG双方完整4字节 |
| 自有C同二进制LAN | 0.0.0.0:60179 →192.168.3.5:60179 | exit1，connect+write4成功，listener poll=0，accepted=0，六秒真实有界失败 |
| Apple签名Python同LAN | 0.0.0.0:60192 →192.168.3.5:60192 | exit0，accepted=True，双方4字节完整交换 |

C SHA=`bae663f002198af37ca27b1c6886dd377aabf17f5eb026dfc3fae0e7193bfe2b`，CDHash=`19a9ba45d913110977ef8c88a59e77d8b04aedd5`。`/usr/bin/python3`是Apple签名工具shim；额外核实sys.executable/xcrun实际解析到CommandLineTools Python3.9，实际文件SHA=`6e7ae61f68a3838094fc56590f84c52069a97d7816f6bb79e0e85995b340e464`、CDHash=`01f9566bbbf6c1e4210ad0b6802acfcb7dd76165`，Apple Software Signing链验证exit0。没有把shim与实际解释器混为同一文件。

r00属于生产crate集成层：合成home/ManualClock/认证数据、真实ServiceState/bootstrap/run/HTTP/TCP；它不是正常service外部启动证明。探针只测试最小网络端点，不替代Lingxi入口、不证明外部手机、真实供应商或其他平台已经验证。

## ALF只读状态与结论强度

前后global enabled(State1)、block-all disabled、built-in/downloaded signed自动允许enabled；全部查询exit0。确切r00路径listapps显示Allow，getappblocked显示permitted，**与本轮同身份LAN红并存**。自有新C没有显式listapps应用条目，getappblocked同样permitted但LAN实测红。不能把文字查询当有效入站PASS。

真实en0=192.168.3.5、en1=192.168.3.10；完整ifconfig、IPv4路由、route get192.168.3.5在前后快照。该本机地址路由为lo0，局域网网段仍在真实en0/en1；存在utun路由不等于该本机地址必走TUN。未改路由、代理、pf或防火墙。

差分强烈支持应用相关的非回环入站过滤，排除Lingxi业务独有失败和同地址普遍不可达；ALF为当前主要环境线索。未读取ALF内部有效规则、未做授权的策略开关差分，因此**不声称路径+CDHash是已经排他证明的内部判定模型**，也不继承RR2对应强措辞或“只剩ALF”的全阶段结论。

## 操作准备、异常和交root

PREPARED-ALLOW.md及prepared-object.json绑定本轮确切对象；本人只读identity_check实际exit0/MATCH。已从本机帮助核对remove/add/unblockapp准确名称，未执行sudo、remove/add/unblockapp、全局关闭、重签或权限操作。

帮助查询真实exit255保留。探针LAN自然exit1和r00自然exit101保留。只读查找误用了不存在的dispatch/request.txt、rust/.cargo/config.toml和verify/runner.rs，三个检查命令真实exit1；后续读取实际dispatch/request.json和verify.rs纠正。证据汇总stdin首尝试另因UTF-8声明缺失exit1，增加明确编码后完成；未重跑套件。这四项均记录在inspection-errors.json，不把查找/汇总错误当产品或gate失败。早期大输出出现截断，权威原文补分段、raw全量读验并保存索引；未伪造未完成门禁的数量。

mustFix（生产源码）：本D范围无新增源码修复要求。remaining（必需外部条件）：由总控先完成其他独立工作，再集中处理本轮确切对象的应用级重新登记允许入站；操作是否有效仍须新编号真实LAN验证。本审查没有权限请求。若FINAL重链接，当前准备立即过期，必须从FINAL CargoJSON重新核身份与操作目标；当前尚未取得FINAL实际目标身份，不能先承诺相同。正式workspace/前序闭包/全部§5.3由新FINAL审查者执行，本轮未运行也未声明通过。

只写RR3/D-REVIEW-01自有证据（Cargo正常构建写target及测试自身合成临时数据另标运行副产物），未修改生产源码、测试断言、门槛、系统规则、其他包、总控docs或Git。完整证据manifest排除自身及父级持续写入dispatch日志；dispatch稳定request/prompt引用另存session-identity.json。定位和准备完整可审阅，**required环境仍BLOCKED / gate FAIL / R06_READY=false**。下一棒交root消费，不自行继续重跑。
