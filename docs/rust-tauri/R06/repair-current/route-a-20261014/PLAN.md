# R06 路线 A 执行断点（2026-10-14）

用户明确选择逐项实现全部 78 个 GAP，优先 26 个仅属 R06 的叶。允许在 codex/rust-tauri-migration 提交并推送；未授权延期、放宽断言、修改门禁判定或扩大白名单。

## 可观察的完成标准

每叶保留原断言，真实成功/拒绝/副作用全部验证后登记 READY；78 叶全部处理、w11 根因查清后才能执行干净全量绑定窗口与正式阶段门禁。只有正式门禁 exit 0 才能登记阶段验收和 R07_READY。当前 R06 未验收，F14 保持开放。

## 当前分工

- 主控：集成、绑定、逐批验证；先补会话笔记错误边界，然后推进其他未分配缺口。
- w11 独立工作树：查清三项 native FIFO 异常，不靠重跑判通过。
- memory 独立工作树：04EE57C5D2BF / 7563265D4241 / DCF0836C3A16。
- persona 独立工作树：人格说明及助手/role 头像八叶。
- 所有分支补丁由主控核对、合并并复验；子代理不提交或推送。

## 待完成清单

以下为起始状态快照，勾选仅表示本轮已实现并验证，不代表正式阶段验收。

- [x] `R00-T02-LA-04EE57C5D2BF`（R06）：写入 active 原则并发 tenets-changed；重复返回 duplicate
- [ ] `R00-T02-LA-0779DAF64B03`（R06）：按精确ID或关键词模糊匹配删除一个或多个active条目，返回removedCount与已删除项。
- [ ] `R00-T02-LA-0B483D7F6A43`（R06）：enabled=false 写 disabledSince 并暂停 ticker；重新启用写 reenableAt 并恢复
- [ ] `R00-T02-LA-206785535A40`（R06）：逐动作核对实际状态与页面投影；失败不伪装成空数据
- [ ] `R00-T02-LA-32E9930CF9D9`（R06）：实时事件驱动情绪/人格状态展示
- [ ] `R00-T02-LA-3516A7326DD9`（R06）：助手先删除；API 返回 skillsDeleted/skillsSkipped。当前 UI 只按 skillsDeleted 数量显示成功 toast，不显示 skillsSkipped；引用清理失败可能只记日志
- [ ] `R00-T02-LA-396F817018F6`（R06）：清空事实库与整理产物并留下重置标记；失败返回错误
- [ ] `R00-T02-LA-4870805E3A3D`（R06）：持久写入正文、刷新描述并广播 agent-updated；/ishiki 同一文件
- [ ] `R00-T02-LA-4A56F26EDA32`（R06）：移除该 role 的自定义头像文件，再读取返回 404
- [ ] `R00-T02-LA-53B672CBEA46`（R06）：清理编译记忆与总结来源并留下重置标记；原始事实库不按此接口清空
- [x] `R00-T02-LA-628005EDFF37`（R06）：read返回文本；write/append写入会话专属笔记，下次读取内容一致。
- [ ] `R00-T02-LA-7432B00DEC94`（R06）：返回匹配事实、时间/标签和检索状态；可选跨频道依配置。
- [x] `R00-T02-LA-7563265D4241`（R06）：删除不在新名单的 active 原则（含模型提议项），新增缺项为 user_direct active，发 agent-updated
- [ ] `R00-T02-LA-7570BE1F2527`（R06）：删除该 role 的旧格式头像并写入新图片，返回实际格式
- [ ] `R00-T02-LA-76EE734926B4`（R06）：返回指定助手修订清单
- [ ] `R00-T02-LA-7C699FD2A740`（R06）：写入 AGENTS.public.md 并广播 agent-updated；兼容旧地址同效
- [ ] `R00-T02-LA-7F0F3F76E38A`（R06）：删除旧格式头像并写入新的图片，刷新助手列表并广播 agent-updated
- [ ] `R00-T02-LA-856B28902999`（R06）：持久写入 identity.md，刷新描述并广播 agent-updated
- [ ] `R00-T02-LA-A02D8C572F5C`（R06）：返回本地头像路径供 splash 渲染
- [ ] `R00-T02-LA-A17D742E55B7`（R06）：经验文档和索引出现新内容，后续recall可读。
- [ ] `R00-T02-LA-A9AD4C7F76AE`（R06）：启动一次手动 dream，返回 202 与运行信息
- [ ] `R00-T02-LA-B09CC9536400`（R06）：读取已保存 MOOD 轨迹并恢复历史展示
- [ ] `R00-T02-LA-B45ECEC543C6`（R06）：返回指定助手 ticker 当前状态
- [ ] `R00-T02-LA-BB4CD1E4CD43`（R06）：删除所有头像格式并广播 agent-updated
- [x] `R00-T02-LA-DCF0836C3A16`（R06）：按 # 标题同步 experience 分类及索引并刷新助手；空内容会清空分类
- [ ] `R00-T02-LA-E4BEE8E6974A`（R06）：返回图片及 ETag；未设置返回 404，ETag 相符返回 304
- [ ] `R00-T02-LA-0DDB5AA05072`（R06 / R07）：逐动作核对实际状态与页面投影；失败不伪装成空数据
- [ ] `R00-T02-LA-0FE8BED686E2`（R03 / R06 / R08）：先回复正在压缩，再返回 token 前后数或完成说明
- [ ] `R00-T02-LA-238267978DDA`（R04 / R06）：旧 IPC 监听注册可发 file-changed；现行 viewer 通过 ResourceIO resource.changed 刷新
- [ ] `R00-T02-LA-2F2B6E173C1B`（R04 / R06）：返回转换的 HTML
- [ ] `R00-T02-LA-2FAA749DA790`（R03 / R06 / R08）：把命令交给同一 slash dispatcher 并以 slash_result 显示
- [ ] `R00-T02-LA-31CDA283A11A`（R06 / R08）：经路径和冲突校验后把归档文件及 sidecar 移回活跃目录并更新生命周期，返回新路径或错误
- [ ] `R00-T02-LA-3617DF5B23D5`（R04 / R06）：返回首张工作表 HTML 表格
- [ ] `R00-T02-LA-3CBCAE9AD914`（R04 / R06）：stat返回元数据；copy在授权工作区落文件；extract返回文档文本与来源。
- [ ] `R00-T02-LA-3FCB259FF69C`（R06 / R08）：删除当前会话历史并提示已清除
- [ ] `R00-T02-LA-4173A08A51CC`（R04 / R06 / R09）：文本/Markdown/Office 等显示相应预览；资源改变事件后重读；可派生只读窗口
- [ ] `R00-T02-LA-421EEA1AC8D5`（R03 / R06 / R08）：先回 compaction_accepted，再按实际结果回 succeeded/noop/failed
- [ ] `R00-T02-LA-4EA88B5073E4`（R03 / R06 / R08）：当前流中止；无活动流时返回已停止提示
- [ ] `R00-T02-LA-4F0466F7C090`（R04 / R06）：文件字节改变并登记会话文件产物。
- [ ] `R00-T02-LA-52A5D1660088`（R06 / R07）：系统目录对话框返回路径，供技能安装/配置后续处理
- [ ] `R00-T02-LA-5A6E0D526903`（R03 / R06 / R08）：刷新提示词和记忆后压缩，回传 token 变化或原因
- [ ] `R00-T02-LA-6207F7E8C697`（R06 / R08）：create保存会话锚点及可选工作区快照；list/drop反映记录变化。
- [ ] `R00-T02-LA-7244E7DCE678`（R03 / R06）：返回当前会话 pending/resolved/failed 项及结果，不跨会话。
- [ ] `R00-T02-LA-771296A194E0`（R06 / R07）：审核后把课程/技能文档写入Agent技能区，可在后续会话检索。
- [ ] `R00-T02-LA-7803B231FC36`（R06 / R07）：调用工具模型，返回目标语言名称映射
- [ ] `R00-T02-LA-7CBF6F4A6760`（R03 / R06 / R08）：已接收中止返回 abort_result accepted；排队提交停止后 UI 立即恢复空闲
- [ ] `R00-T02-LA-81664285F927`（R03 / R06 / R08）：按 streamId/sinceSeq 补发遗漏事件和重置/截断标记
- [ ] `R00-T02-LA-81B217A7E581`（R03 / R06 / R08）：逐动作核对实际状态与页面投影；失败不伪装成空数据
- [ ] `R00-T02-LA-84E22CD74BE1`（R03 / R06）：任务处理器接收stop，任务状态转终止且回报；支持子代理/媒体等。
- [ ] `R00-T02-LA-857747F848F6`（R03 / R06）：按 taskId 与所属会话停止任务，返回 subagent_stop_result
- [ ] `R00-T02-LA-8A69F0CFFFA1`（R06 / R07）：安装包至 .agents/skills，刷新技能路径，返回 name/installedSkillSource
- [ ] `R00-T02-LA-8E2D44B48905`（R06 / R08）：逐动作核对实际状态与页面投影；失败不伪装成空数据
- [ ] `R00-T02-LA-92620BF2264F`（R03 / R06 / R08）：经会话授权和媒体校验后走插入队列/打断策略，收到输入回执和新回复
- [ ] `R00-T02-LA-93D5237ADE81`（R04 / R06）：返回文件内容与来源；图片可走视觉桥，Office可抽取文本；不改原文件。
- [ ] `R00-T02-LA-94CB746D04B7`（R03 / R06 / R08）：输入区出现一张批量提问卡；确认返回按key映射的答案与answered=true；超时只自动填推荐项并标timedOut/autoSelected；拒答返回dismissed且answers为空。
- [ ] `R00-T02-LA-980C099F356F`（R03 / R06）：默认关闭；开启后新会话主 Agent 提示增加独立研究可用只读子代理的条件，工具依旧按权限执行
- [ ] `R00-T02-LA-99F8CD228733`（R06 / R07）：逐动作核对实际状态与页面投影；失败不伪装成空数据
- [ ] `R00-T02-LA-9CA8A47BE208`（R06 / R07）：独立技能查看器列文件并显示所选文本
- [ ] `R00-T02-LA-A2C385A33493`（R03 / R06 / R08）：悬浮窗口可见/尺寸改变；选择会话聚焦主窗并打开该会话
- [ ] `R00-T02-LA-AD7FAE6288FA`（R04 / R06）：本地资源返回真实路径；远端资源先暂存本地副本再返回路径与resourceKey，不修改原资源。
- [ ] `R00-T02-LA-AF18F5CEA14F`（R04 / R06）：合法目标存在时 shell.trashItem 后书桌列表更新
- [ ] `R00-T02-LA-B34F2B298BFA`（R03 / R06 / R08）：会话流发出完整新清单，界面显示各项pending/in_progress/blocked/completed状态。
- [ ] `R00-T02-LA-B47CAC08F418`（R03 / R06 / R08）：返回 aborted 或 already_stopped；找不到 404，非本会话拥有 403
- [ ] `R00-T02-LA-B97D0D4296FA`（R04 / R06）：返回目录项与元数据，不改文件。
- [ ] `R00-T02-LA-B9DA1A584C6E`（R06 / R08）：按指定 Agent、工作区、记忆和模型设置新建焦点会话，写元数据并广播
- [ ] `R00-T02-LA-BA490D930CA9`（R06 / R07）：重新扫描所有技能并广播 skills-changed；前端再按 agentId 取列表
- [ ] `R00-T02-LA-BB0A168006D7`（R06 / R08）：只接受绝对文件路径和 edit-start/autosave-interval 原因，返回新检查点
- [ ] `R00-T02-LA-BDE3AC44D79C`（R04 / R06）：只允许 cut/copy/paste/selectAll，并向当前 webContents 派发该命令
- [ ] `R00-T02-LA-C1FB9C92E813`（R03 / R06 / R08）：活动流被 steer 后收到 steered；流已停则作为普通 prompt 提交
- [ ] `R00-T02-LA-D483E4CD0935`（R03 / R06 / R08）：经授权进入会话 Run，流式返回阶段、工具和最终回复
- [ ] `R00-T02-LA-D8D47C4680C9`（R03 / R06）：逐动作核对实际状态与页面投影；失败不伪装成空数据
- [ ] `R00-T02-LA-D901852E1B1A`（R03 / R06 / R08）：按 sessionId 保存；sessionPath 先解析为 sessionId
- [ ] `R00-T02-LA-E1625F70C7CC`（R06 / R08）：用户批准后回退会话分支；繁忙时延后到本轮结束并回报；文件恢复受版本检查。
- [ ] `R00-T02-LA-EC77C2B04726`（R04 / R06）：返回匹配行/位置，不修改文件。
- [ ] `R00-T02-LA-EE2ACB390FCE`（R06 / R07）：生成cardId并把title/code存入会话结果，前端在隔离iframe渲染卡片。
- [ ] `R00-T02-LA-F88B0A969094`（R04 / R06）：返回路径列表，不写文件。
- [ ] `R00-T02-LA-FBDEEDAC0B11`（R06 / R08）：改变当前会话并尝试恢复该会话浏览器，旧会话浏览器不随切换挂起
- [ ] `R00-T02-LA-FF4F06011B48`（R06 / R08）：读取会话 sidecar 中冻结的系统提示词和工具名

## 阶段审查另列义务

- [ ] EX-01：手动压缩持久提交、下一 Run 消费、真实 enabled/keepRecent/reserve、自定义关注。
- [ ] 4F0466F7C090：OS 沙盒条款真实证明。
- [ ] 31CDA283A11A：R06 sidecar 路径和冲突检查。
- [ ] 其他 EX-02–EX-12 当期核心份额逐项核实及验证，不能由叶登记替代。

## 已知不可跳过的约束

仅属 R06 的清单包含真实设置页、MOOD 实时/历史、splash 与 ticker，不能仅完成后台部分就翻 READY。前序或后继阶段有份额的 52 叶也按 A 路线实现，不自行转延期。门禁运行期间工作树冻结；只留真实运行证据，不把本地通过当成多平台或真实供应商验证。

## 首批验证断点

4 叶已完成并绑定，205 READY / 74 GAP；新窗口 7 绑定及四叶rollup PASS；W11 五旧 native 绑定与新增读者隔离绑定共 6 PASS；主控整合后三套记忆/上下文回归 47 PASS，登记契约 20/20。W11 的具体历史 reader 身份仍未知，受控同用户reader精确复刻失败，0200权限阻止该干扰；不宣称识别了历史 reader。证据见 artifacts/rust-tauri/R06/route-a-20261014/batch1-validation.json。正式门禁未重跑，R06 未验收。
