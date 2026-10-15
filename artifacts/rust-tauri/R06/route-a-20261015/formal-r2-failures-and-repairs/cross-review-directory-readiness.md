# 系统目录选择窗口等待条件：独立只读复核

结论：PASS（本补丁只读复核及 Root 本轮真实 GUI 原始证据审阅），无需修项。原业务断言和超时全部保留，本次真实父子测试均完整通过；原失败保留。Reviewer 没有修改 Main 或重跑任何测试/检查。

## 根因有真实失败和源码顺序支撑

旧绑定 /private/tmp/r06-targeted-formal-r2-repairs/r06_test_9b21a2d853775007038f/binding-result.json 真实 exit101、sourceStable=true、passed=false；子 stderr 明确 directory_picker_host.rs:187:42 Option::unwrap None。原取消结果为 null、输入仍为原路径的断言在 notice 之前，执行已越过；前一选择阶段亦已通过并保存截图。原失败没有改算 PASS。

生产 bridge.ts:55–58 在 await core.invoke 得到真实结果后，同步 dispatch lingxi-resource-response，然后才 return result。App.tsx:88–91 在外层 await bridge.selectFolder() 恢复以后才 setSkillPath/setNotice，React 仍需提交 DOM。监听器已把结果追加到 __directoryResults，并不等于 role=status 已渲染，故旧仅检查数组长度的谓词可先取到 notice 缺字段的对象。WebDriver 对象缺 notice 后，Rust Value 读取为 null、as_str 返回 None，匹配真实崩点。此诊断不是凭空把产品返回问题改称测试问题。

## 净改动和合同逐字保留

当前 desktop/src-tauri/tests/directory_picker_host.rs SHA256 为 6f1a6a4b2d7717b803ce0eee3eae1509d6dca60fb480d0dc20fec3c4dde2fd79。独立将两新谓词换回输入文件对应旧谓词、去掉两行新中文注释，即完整逐字还原 HEAD 原文件。不是只数 assert 数量：全部业务断言、请求/命令、界面操作、原生窗口路径、文件检查、证据写入位置、超时与后续只读窗口拒绝路径都原样保留。两个 Duration::from_secs(240) 仍各在原等待处；driver 的100ms轮询、40秒请求、其他窗口超时及外层原900秒绑定亦无变更。bridge.ts / App.tsx / 父r06_directory_picker.rs 均逐字等 HEAD，无生产代码改动。

新选择谓词仍要求 length===1，增加 DOM status 实际包含“尚未安装”才截取原 result/input/notice 三字段；新取消谓词仍要求 length===2，增加 DOM status 实际包含“已取消”。选/取消提示不同，取消开始时原 action 也清空 notice，不能借前一次“尚未安装”提示过关。没有改写结果、输入或提示，没有匹配其它事件假冒原生回包。真正返回错路径、取消非null或输入被清掉，原值断言仍会失败；无提示/错误提示会保持等待并在原240秒上限内失败。不是把错误分支 filter 掉而返回成功，也没有 ignore/skip、catch异常转成功、替换 driver 协议错误判断、预填 receipt 或删除失败断言。

selection-and-cancel.json 仍在选/取消结果、输入、提示、源字节/目录数量与未安装检查后才写；最终 receipt.json 仍在另开只读窗口、select_folder被拒绝、源字节及未安装末检通过之后才写。choose-ready/cancel-ready 仅供真实系统操作协调，不是验收收据。

## 六项确定性对照的准确边界

已读 /tmp/r06-directory-render-readiness-input.json，old/new 字符串与当前真实修复一致；已读 probe.json（6 passed/0 failed）：两阶段各有“事件到但界面未提交时旧早返回/新继续等”“错误提示继续等”“预期提示真实就绪后保存全部原字段”三种对照。该对照支持只收紧就绪判据，不代替真正系统目录对话框/取消或源字节验证。本 Reviewer 没有重新运行对照脚本。

## 本轮真实 GUI 完整通过记录

已实际读取同一 fresh 目录 /private/tmp/r06-targeted-formal-r2-directory-ready-r2/ 的 binding-result.json、父 stdout/stderr、proof 子 runner-stdout/stderr 与最终 receipt.json，未使用旧历史或别处收据替换。本次绑定 key=r06_test_9b21a2d853775007038f，sourceCandidateSha256=8e6b91b4cd2f700c572854bf7bc0604f45e27bdafd26c27117fb5be1f6e750b8，前后 sourceStable=true，actual exit0 / timedOut=false / outputCheck.valid=true，父精确原测试1 PASS，实际命令112.635秒。子 native_directory_picker_returns_real_path_cancel_null_and_never_installs 原测试1 PASS，99.45秒。原 outer900秒/两段240秒均保留；不是0匹配或ignore。父stdout/stderr实际字节SHA也匹配binding记录。

最终 receipt 实际记录 selected.result 与 selected.input 均等本轮 selected-skill 路径、notice包含“尚未安装”；cancelled.result=null、input仍同一路径、notice为“已取消选择目录”；readonly.ok=false，原生 capability 错误明确 skill-viewer-0 无select_folder权限、仅main允许。源字节/未安装等原断言由实际完整父子成功证明，且当前最终receipt写入位置在所有末检之后。原旧 source=22e0c9ce...f275 的真实失败仍保持 exit101，不能用新 source=8e6b...0b8 通过覆盖它。

当前测试源码SHA仍为已审6f1a6a...fd79，未在复验后被替换。机器收据包含新候选中该测试条目、真实GUI原始日志/receipt SHA与完整结果。此处是 Reviewer 独立读取 Root 实际操作和运行记录，不冒充 Reviewer 再次亲自开窗口或重新执行；binding自己的stageVerdict仍为NOT_A_STAGE_VERDICT。

本次只读源码/现有证据，唯一写入本报告目录；未操作系统窗口、未修改检查规则/白名单/阶段输入、未提交推送。不扩大到 Windows/Linux 或全阶段验收。
