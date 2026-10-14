# R06 情绪两叶精确候选独立审阅

日期：2026-10-14。结论：**NEEDS_FIX**。确认一项分页游标缺陷，分别由真实HTTP和当前生产组件真HTTP+真WebSocket复现。已有小规模情绪功能与原正文保护测试通过；不能据这些PASS登记两叶完成或R06_ACCEPTED/R07_READY。

## 对象与隔离

- 仅叶 `32E9930CF9D9`、`B09CC9536400`。完整原assertions从现行 `scripts/rust-tauri/r06_original_leaf_bindings.json`读取：前者“实时事件驱动情绪/人格状态展示”“断连或没有 MOOD 事件保持空态/旧态，不当成已写历史”；后者“读取已保存 MOOD 轨迹并恢复历史展示”“无历史轨迹时空态，不新造实时 MOOD”。
- detached基座 `d73729a2821a51e39e27038c0c42052a1f2f737e`，独立树 `/private/tmp/lingxi-r06-review-mood`。精确应用 `/tmp/lingxi-r06-mood-trajectory.patch`，SHA256 `0f3b49aecdce393fcd6a72ebb94966bb7bb2472c9c1b57e285d57e21ae7623a3`；23个候选文件在应用后与验证结束时都匹配 `/tmp/lingxi-r06-mood-source-sha.json` 的全部SHA。没有从作者测试结果自动签PASS。
- 首次完整worktree因磁盘空间不足未能建立，git清理失败副本后改用不包含庞大artifacts目录的稀疏独立worktree；生产、正式测试、原叶绑定均真实完整存在。没有copy target或.git，没有修改Main或作者候选；没有提交/推送/改登记、门禁或原断言。
- Node实际v24.16.0；仅symlink复用既有node_modules依赖。正式cargo绝对路径 `/Users/study_superior/.cargo/bin/cargo`、1.98.1、--locked、CARGO_INCREMENTAL=0。合成HOME/LINGXI_HOME/TMPDIR/产品根 `/private/tmp/r06-mood-review-runtime`。target `/Users/study_superior/.cache/lingxi-r06-builds/reviewer-target` 在/private/tmp与TMPDIR允许范围外；没有读取真实业务HOME数据。

## P2 确认发现：情绪HTTP续页游标误用真实session ID，服务拒绝自己的游标，记录较多时界面不能恢复

定位：`rust/crates/lingxi-service/src/events.rs:1455-1460`。`events_page_inner` 的 `stream_id` 参数是session ID，`subscribe_mood`已转换成独立情绪流 `lingxi.mood/sha256(session)`。请求limit小于实际返回页时重新构造nextCursor却用了原参数stream_id，导致该游标下一次被 `subscribe_authorized` 的流绑定检查拒绝。

修复方向：截短页后的游标仍绑定 `cut.stream_id` 的实际流，不能为达到200绕过游标绑定或权限校验，也不能放宽原正文保护断言。

### 独立真实HTTP反例：FAIL

`reviewer_mood_http_page_cursor_remains_bound_to_actual_mood_stream` 用生产供应商请求与真实认证HTTP execute持久化一段情绪；GET mood-events?limit=1实际得到首事件、streamId=lingxi.mood/hash。服务返回nextCursor内部s=sess_local_alpha；再用原样游标向同接口请求续页，实际 **HTTP400 / causeId events.stale_stream_cursor**，消息明确“cursor was issued for stream sess_local_alpha, not lingxi.mood/hash”。检查期待服务自己发出的游标可续读200，实际400导致FAIL。

日志 `/tmp/r06-mood-review-page-counterexample.log`。探针源码 `/private/tmp/lingxi-r06-review-mood/rust/crates/lingxi-service/tests/r06_review_mood.rs:535`。

### 独立当前生产组件大轨迹反例：FAIL

模型替身仅提供一次真实模型回答：300个 `<mood>轨迹NNN</mood>`；生产扫描器、Run提交事务、独立持久流和真实历史均未替换。真实mood-history返回300个已保存完整段（约900个start/delta/end事件）。

随后挂载未修改的 `RustMoodPanel`，仅在登记表设置连接（activeServerConnection为空），使用生产resolveServerConnection/fetch和ws网络transport。WS首500事件后HTTP续页默认200；第一次HTTP续页截短后发出错误nextCursor，下一页400。实际DOM只剩 **“情绪历史未恢复：读取失败（400）”与“暂无情绪轨迹”**，300段保存轨迹一个也没有显示。子测试严格JSON为1 total、0passed、1failed、0pending，父测试也FAIL；不是skip或模拟组件。

这证明错误实际影响两叶中的历史恢复与重连/快照合流，而不是仅理论上的额外HTTP limit。文件和服务内历史未丢失，失败发生在展示恢复。无需重复扩查同一个已确认错误。

探针父源码 `/private/tmp/lingxi-r06-review-mood/rust/crates/lingxi-service/tests/r06_review_mood.rs:550`：`reviewer_current_mood_ui_restores_paginated_real_trajectory_without_cursor_failure`。子代码 `/private/tmp/lingxi-r06-review-mood/desktop/src/react/components/chat/__tests__/R06ReviewRustMoodLive.test.tsx:11`：`reviewer_mood_paginated_history_actual_network`。大回答分支名REVIEW_MANY_MOOD；外部网络回答替身的这个分支不修改生产scanner/service/storage/UI。

日志 `/tmp/r06-mood-review-ui-pagination.log`。

## 已实际通过的验证

- 精确候选 `r05_t04_streaming` **18/18PASS**：保留原保护断言，包括MOOD-only不造最终正文、同一扫描器实时与历史规范化、真实取消、供应商失败/重试身份。
- 精确候选 `r06_t04_history` **7/7PASS**：原历史分页/重连/导出、权限、分支、run关联、终态等断言未改。
- 精确候选 `r06_mood_trajectory` **3/3PASS**：真实重启恢复、空历史读取无写事件、未闭合段实际取消后重启仍partial、其它主体HTTP拒绝、普通订阅不可读取保留情绪流；包含当前组件真HTTP+真ws的小历史实时/断连旧态/重新挂载恢复/会话切换空态子例，父严格JSON单例通过，无pending。
- `streaming_norm::tests` **18/18PASS**：真实分块MOOD、真实闭合、代码/转义/隐藏标签、无MOOD、未闭合不造end，原普通正文词法保护等。
- `git diff --check` PASS；23文件SHA重复一致。

总计46个顶层/单元检查PASS，另2个独立父反例FAIL（大UI内含1个真实网络子例FAIL）。原测试的小样本PASS无法覆盖上述跨多页快照失败。

## 其它重点与限制

读取生产source确认保存Mood沿key_events同事务commit后publish，普通流last_event_seq不被Mood独立序号覆盖；仅真实同名关闭标签生成Mood end，model-call SHA进入segment ID；普通订阅拒绝保留流，Mood订阅授权实际session；seed/create/fork三个写入口拒绝保留命名空间。已测真跨主体读取、取消partial和普通订阅拒绝；本轮没有另做真实seed/fork碰撞写入反例，没有另跑token/registry连接切换的恶意竞态，没有重跑root TS/clippy。登记表单独连接选取已用于大UI真网络探针，但其记录恢复被当前分页问题阻断，不能把该失败说成完整连接切换隔离已验收。

没有测试真实Electron/Tauri窗口或全部聊天可用性，没有跨平台、真实收费供应商或正式打包证明。旧正文/历史断言未修改。所有反例都仅写独立审阅树的新测试文件，原23候选文件冻结不变。候选Root正在据实际证据准备R2；本报告只评价冻结R1，不预先签修复通过。

## 执行命令

所有cargo命令使用前缀：

```sh
env CARGO_INCREMENTAL=0 HOME=/private/tmp/r06-mood-review-runtime/home TMPDIR=/private/tmp/r06-mood-review-runtime/tmp LINGXI_HOME=/private/tmp/r06-mood-review-runtime/home LINGXI_PRODUCT_DIR=/private/tmp/r06-mood-review-runtime/product CARGO_HOME=/Users/study_superior/.cargo RUSTUP_HOME=/Users/study_superior/.rustup CARGO_TARGET_DIR=/Users/study_superior/.cache/lingxi-r06-builds/reviewer-target /Users/study_superior/.cargo/bin/cargo
```

在上述独立树执行参数：

```text
test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r06_mood_trajectory --test r05_t04_streaming --test r06_t04_history
test --manifest-path rust/Cargo.toml --locked -p lingxi-service --lib streaming_norm::tests
test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r06_review_mood reviewer_mood_http_page_cursor -- --nocapture
test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r06_review_mood reviewer_current_mood_ui -- --nocapture
```

回归日志 `/tmp/r06-mood-review-exact-tests.log`，解析日志 `/tmp/r06-mood-review-parser-tests.log`。候选SHA、探针SHA、精确失败名称与scope JSON保存在 `/tmp/r06-mood-independent-confirmation.json`，独立工作树/运行目录/target/日志保留供Root修复后同一审阅任务复验。
