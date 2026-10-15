# 实验设置 / 主动委派两叶净交付

- 范围：D8D47C4680C9、980C099F356F。仅隔离目录工作；未改 Main、登记、提交或推送，未加依赖或改门禁。
- 净补丁 experiments-two.patch，20 文件；逐文件 base/candidate SHA 和集合 SHA 在 experiments-two-manifest.json。净补丁已在仅含基线文件的独立目录严格回放，20 个最终字节 SHA 全相等（experiment-net-replay.json）。
- committedBase 是 a477ed69f485f43df2c1c5fb4dc6b1ca57cf3633；它不是整包唯一前置。另有 Root 已合的交互19/会话24/目录源码及聊天共享文件，精确前置见 experiment-rebase-prerequisites.json。session_tree 两文件另以会话作者 map-fork-runtime-prerequisite 为前置，保留其 PreparedForkUi 和 memory/project 继承；adapter 叠加已交交互19 atomic append helper（experiment-map-prerequisite.json、experiment-interaction-adapter-prerequisite.patch）。这些前置不包含在本包净变化。

## 实现

- settings/api 只追加 experiments 与 preferences/computer-use 已注册路由映射。当前四个实验目录来自实际偏好；一次读取共享同一份偏好，坏文件响亮失败。保存后才发布 experiment-changed；失败零通知。settings snapshot 供现役组件直接恢复当前实验与 Computer Use 资料。
- Computer Use 保存默认关闭偏好、读取既有批准记录、真实 macOS AX/屏幕录制查询与用户按钮触发的申请。申请返回实际 bool 和 requestPending；不替用户授予系统权限。当前控制引擎缺失明确 available=false / computer_control_engine_unavailable。批准/撤销仅保留服务器接口，页面零调用、只显示既有列表。
- V14 仅增加 sessions.proactive_delegation 默认 false。SessionAdmin 和 focused 创建通过同一 prepared INSERT 冻结当前值；fork 沿唯一原 INSERT 一并保存新会话创建值。旧数据库行保持 false，改变全局开关不改旧会话。正式 compile_session_snapshot 从持久行读取，子代理强制不注入。真实权限列/授权目录/写者检查未放宽。

## 原始验证

- experiment-baseline-evidence/raw-failure.log：还原精确前置，旧会话在打开全局开关后错误获得提示，真实断言失败、退出101。experiment-baseline-run.py finally 恢复候选；result.json 保存结局。
- experiment-final-frozen-parent.log：完整5父 PASS，11.52s：默认/旧/新/子代理、真实已登记 subagent 的 write 权限拒绝且零 child、观察记录持久读/删及403/IO失败、偏好坏文件/写失败零通知、真正启动CLI+focused/fork创建+9条模型请求/账本，以及真实Tauri当前生产组件。
- 最后只清理 Python 行尾空格，受影响的 actual_cli_focused 父已单独重跑 PASS 8.08s（experiment-final-cli-format.log）。其他生产源及GUI源与完整5父时逐字相同。
- 设置父进程下4个Vitest文件 18/18 PASS：原 R02SharingLeaf、ExperimentsTab、ComputerUseSection，以及新增真实Rust HTTP页面。原断言不变；包括冷启动、缓存、15秒、初始化异常/供应商不阻塞。
- 真实GUI：/private/var/folders/5r/wq54gtms21gfjgr5v94ynfyh0000gn/T/lingxi-experiments-completion-4f66e2e487bfd347/home/native-ui-evidence。临时宿主页导入当前生产 ExperimentsTab/ComputerUseSection，在真Tauri WebView操作真实服务；9组检查、两张PNG、driver/原生日志/HTTP状态/原始收据。已亲读两PNG。保存/申请403、PATCH失败无广播、通知失败无成功toast、保存后GET401快照回退、实际申请后重新GET，均硬断言。
- 真实系统状态：申请前后 accessibility=false、screenRecording=true；requestPending=true，权限对象明确 service_process。没有点击允许或更改系统授权，不把未授权当成功。
- 实际CLI最新收据：/private/var/folders/5r/wq54gtms21gfjgr5v94ynfyh0000gn/T/lingxi-experiment-focused-a17f8984bd302f09。raw-provider-requests.json 保存真实HTTP替身收到的请求，关闭全局开关后新会话冻结提示仍在发出文本；focused、detached 和两种 fork 创建值分别核对数据库，9条模型账本真实落库。未调用付费供应商。
- experiment-service-clippy.log：service lib+新父严格 Clippy PASS 15.50s；experiment-native-clippy.log：native e2e父严格 Clippy PASS 2.53s；experiment-typecheck-final.log：定向TypeScript PASS；experiment-migrations.log：原迁移12项 PASS，原迁移指纹/防降级检查保持。

## 完整原文与边界

experiment-original-leaves.json 保留两叶全部原断言。五组逐项对应：设置容器原父；四实验默认/目录/快照与原页面；保存+广播及四类真实失败；真实观察文件读/删/空态/拒绝/IO失败；Computer Use真系统查询/申请与只读批准列表。

cache snapshot 在现役目录本来已退役，不重新注册它。真实窗口依原“未注册不显示”核对无卡片；观察GET/DELETE仍用真实文件验证；原React shadow/write条件分支保持原实现，不把不存在的当前卡片说成当前运行成功。Windows/Linux GUI、系统权限运行 NOT_RUN；对应平台不支持申请时显式失败，未包装 unsupported 为成功。本包未实现完整电脑控制引擎、付费供应商、完整发布打包或全主窗口布局验收。

Apple 主源：
https://developer.apple.com/documentation/applicationservices/1459186-axisprocesstrustedwithoptions
https://developer.apple.com/documentation/coregraphics/cgpreflightscreencaptureaccess()
https://developer.apple.com/documentation/coregraphics/cgrequestscreencaptureaccess()
本机SDK AXUIElement.h/CGWindow.h 同步核对 Boolean/CFDictionary 与当前进程/异步提示返回值约定。

早期失败保留：缺少合成yuan模板、断言错误归一化预期、查询错表、错误地把未注册subagent失败当权限证明，均被后续加强断言抓到并修正夹具；最终父验证真实登记/权限拒绝并通过，不以先前失败代替最终结果。
