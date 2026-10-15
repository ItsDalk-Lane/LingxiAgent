# 会话首四叶交付（作者验证通过，待主控整合及独立复核）

补丁：`session-four-leaves-final.patch`，28个明确源码/测试文件。SHA256 `05fa12702edb8645c82ab7722427a032c27a30786598cfa89798679fa461ff2c`。逐文件基线及候选见 `session-four-leaves-final-source-sha.json`；基线集合 `25f11cf9e2fd2fd77932722244d94909be222df9de5fc77f61e87a777fa4b920`，候选集合 `efe909936b551895113702010ed4cccc31c83b85d70ea15ae3af8512df6beece`。已对当前Main只读 `git apply --check` 通过，未应用、提交、推送。HEAD 4cdb仅信息坐标，实际基线含当时Main未提交的既有前置，须以逐文件SHA为准。

| 原叶 | 本次实际证据 |
|---|---|
|6207F7E8C697|正式模型目录注册checkpoint，真实mcp发现/调用完成create/list/drop；删除缺项与空白名字真实Failed；read_only允许list但create在网关零派发；正式注册网关无会话create/list、无用户锚点均错误，检查点及文件版本零写。|
|BB0A168006D7|专属user-edit接口只接受绝对文件和两种原因；实际二进制字节备份并经原restore恢复；相对路径、非法原因、缺失、目录、无权限及磁盘拒绝明确失败，无残留新存档。|
|E1625F70C7CC|模型risk_accepted不能替用户确认；生产React页面真实拒绝保留分支/文件，确认时实际Run仍占门只排队；结束后原事务回退真实分支，仅余正确reset锚点；助手写v2恢复v1，外部改动保持并列conflicted；真实持久最终回执再显示完成。|
|FF4F06011B48|实际ModelTurnInput发送边界冻结到专属sidecar；查询只读sidecar，真实provider请求system/toolNames逐字对应，跨会话/主体拒绝、重启读回、损坏文件500不伪空。|

最终原18 + 新5 = 23通过，日志 `session-first-final-23.log`。此前受影响旧网关13、map/checkpoint3、memorytools10、session-tree14 = 40通过，原测试文件及断言未改，日志 `session-first-combined.log`。最终严格Clippy（含新test）通过 `session-first-final-strict.log`；根TS通过 `session-ui-ts-final.log`。

新父测试真实CLI家目录 `/private/tmp/lingxi-r06-agent-delete-env/tmp/lingxi-r06t03-svc-production-checkpoint-model-73513-1792030547584506000`，供应商实际请求45次与SQLite model_call_usage 45条一一对应，embedding=0；10次模型工具调用及批准/拒绝完整走实际生产链。真实Electron包含8项可观察检查：ready, rejected, confirmed, scheduled, completed, isolated, delay-a-held, session-delay-isolated。4次真实批准POST均在正确服务；另一真实服务同ID会话零批准POST、零Run。

## 真失败及修后证据

* 原缺失接口：`negative-1792027523753483000/result.json` user-edit/prompt-snapshot真实404，草稿legacy400。原检查点模型目录未注册的判断来自原源码，不冒称已跑旧模型反例。
* 同服务A迟到回包：`same-session-delay-negative2/review-ui-failure.json` 真实按钮disabled被旧finally解除；`negative3`以同一状态的aria-busy再次真实FAIL。负控Panel取此前原交付源码，仅加可观察DOM属性，生产批准处理未mock；Chromium只暂停真实HTTP响应。修后同脚本 `same-session-delay-fixed/result.json` PASS，B仍busy直到B自身响应，旧A无新增POST/不覆盖B。
* 跨连接与registry A→B→A：同一真实窗口保留旧按钮并点击；两真实CLI同ID服务，B零POST，render不显示旧owner确认。
* 草稿附带回归：`session-five-original-combined.log`未知稳定ID误400（应404）；`combined2/3`非法legacy定位误500（应400）；按原断言修回404/400，真正DB故障继续500，最终原18全通过。全部失败日志原样保留。
* 初次UI设置失败（固定5173占用、调试器在空页面等待）属于夹具失败，已改真实系统分配端口与先load再启调试器；不列作产品失败。一次漏target产生297MiB自有缓存已官方cargo clean清除，留清理日志；后续仅单个仓库外target。

## 共享边界和限制

`runs.rs`只含四处必要共同片段：compile_session_snapshot、真实ModelTurnInput落sidecar及取消重验、prepare同冻结workspace、TrustedToolLifetime同cwd。聊天作者已明确将它们和ChatPage ReviewPanel import/mount当外部前置，从聊天净增量排除。`session_ui.rs`及filetools/exectools是实际冻结cwd消费配套；并无focused-create已完成宣称。`input_drafts`/locator/前端debounce连接保护是共享前置，D901尚不登记。下一包继续focus/browser/map及完整会话UI，不混入此补丁。

本次实际宿主是macOS当前React+Electron生产面板；Tauri会话窗口、其它平台及真实外部付费供应商未验证。stage=prepared_model_call准确标注发送前冻结，取消可能阻止随后发送。保留原历史opaque、原事务与恢复机制；没有修改台账、注册器、规则、门禁、白名单、既有测试断言或Main。指定Cargo1.98 --locked、CARGO_INCREMENTAL=0、合成LINGXI_HOME/TMPDIR；当前单个target `/private/tmp/lingxi-r06-agent-delete-target`。
