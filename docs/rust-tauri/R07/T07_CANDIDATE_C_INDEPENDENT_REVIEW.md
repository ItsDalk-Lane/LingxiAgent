# T07 候选 t07c 的独立复核

状态：`SOURCE_RECHECK_COMPLETE_GATES_FAILED_AND_PENDING`。本审阅者未参与产品运行代码修复，只读检查现有源码与证据；未运行 Cargo、未修改产品源或门禁、未提交推送。旧独立审阅只作定位线索，以下结论均另行核算。

## 版本与归档

独立读取全部 543 个清单项目，逐项对照当前文件与归档内的字节、长度、权限模式，并检查归档没有多余成员；与固定候选完全一致。独立枚举当前 Git 差异内的源文件，没有发现清单遗漏。

- HEAD：`0c86675d49442fe0e38259bd3097423fb340eecc`。
- 源码指纹：`f634f22f7b072c7d258b6dfaf48014c5d0a863ee8aa0fab3f123203a94bbcabe`。
- 归档 SHA256：`34e1ab9afb13b51f849cb83ce0372b696a70c47da1f408f5d9567fbaa6b64e30`。
- 独立原始核对：[independent-t07c-source-audit.json](../../../artifacts/rust-tauri/R07/T07/independent-t07c-source-audit.json)。

## 四个变更文件复查

从 b 的归档提取旧内容；b 未列的两个项目另取 HEAD 原内容，与 c 的归档直接比较。差异只有以下四个文件。

1. `agent_memory.rs`：实际原子写入使用进程编号、单调原子序号和系统时间共同命名；`create_new` 拒绝占用别人的临时文件。写失败只清理本次已成功创建的文件，完整写完并关闭后才 rename。成功不会暴露半份正文。该改动不宣称增加断电持久性，也没有改变原来的“最后一次 rename 获胜”语义。
2. `agent_deletions.rs`：全局单槽改为按 home 分组、每个 guard 具有唯一身份的注册栈。释放旧 guard 只删除自己的注册；不会清除新 guard，也不会复活已释放的旧 guard。回调执行以及捕获对象的析构都在锁外。四个新增检查覆盖不同 home 同时存活、同 home 两种释放顺序和回调重入。没有新增 HTTP 注入入口。
3. `workflow.rs`：只增加真实失败原因的诊断记录。节点对外文案仍使用同一 `reason_segment`，没有放宽现役重试分类或吞掉失败。所记录的是封闭的 `FailureCause`；没有添加模型正文或认证数据字段。
4. `tests/r07_workflow/mod.rs`：原 attempts=1 的断言保留，仅补实际节点及进展信息。另加 16 个真实系统线程、每轮屏障同步、128 轮的现役 IO 检查，总计 2048 次写入；检查零 IO 失败、最终 JSON 完整属于一个真实写者、没有临时文件残留。

本次源码复查未发现上述四项尚未修复的可复现正确性缺陷。此结论不等于已经证明旧扇出间歇失败只有原子写碰撞这一个原因：旧运行只留下 provider_error 与 attempts=2，原始原因不能从事后成功推导。该历史失败必须保留，后续重跑另记。

## 责任边界

直接与冻结的 `R07_LEAF_MAP` 重新比对，T07 任务集合正好 87 项，没有遗漏或多算：65 项是本次责任，22 项是 T03 继承，不重复列为新增完成。重新定位 86 个当期责任测试，记录其真实定义、源摘要、断言位置及路由；逐叶要求与责任测试关联另存 [independent-t07c-leaf-responsibility.json](../../../artifacts/rust-tauri/R07/T07/independent-t07c-leaf-responsibility.json)。定义存在本身不能记作运行通过。

文件、便笺、目录、回收、权限、实际模型账本、循环与工作流检查均有真实服务/磁盘/任务结果断言。Git 使用临时仓与本地 bare 远端；页面映射及组件检查不代表完整原生 Git 面板接真实外部账号。真实服务页面检查从空 home 启动服务，通过正式接口创建助手，再调用实际页面动作并读回文件；heartbeat 与 AGENTS 注入开关来自真正组件点击。它不替换 fetch 或服务端，但缺少 `LINGXI_R07_SERVICE_BIN` 时明确 skip，最后必须检查实际运行行与新构建二进制摘要。

A14 旧兼容字段和单键操作的证据，不代替 T08 的正式导入导出联验。两个可见浏览器/人工操作检查本候选未执行，不能算通过。其他系统、正式包和真实账号仍不在本地证据证明范围。

## 当前门禁事实

已独立核 fmt、clippy、adapter、kernel、protocol、boundaries、types、protocol-tests 八份回执的实际命令、前后源码、HEAD 和完整日志摘要，均匹配固定候选；其中七项原始退出为 0，adapter 退出为 101。

adapter 的真实 Chromium 套件中 `trusted_click_discovers_a_real_popup_and_dialog_events_do_not_deadlock` 在通用 execute 的 unwrap 返回 `browser_closed`，`effect_unknown=true`，该套件 9 过 1 失败；原始失败保留。adapter 日志中还明确 ignored 13 项，不计通过，其中两项需要可见窗口/人工操作，其余十一项需另行显式执行。service、build、ui 的最终回执尚未独立核验。详见 [independent-t07c-gates-partial-audit.json](../../../artifacts/rust-tauri/R07/T07/independent-t07c-gates-partial-audit.json)。

因此，当前不能把 t07c 或 T07 登记为最终通过，更不能称 R07 完成。等待当前组结束、新失败诊断、适用的真实 Chromium 显式检查及使用新二进制的页面整合完成后，再进行最终独立核验。
