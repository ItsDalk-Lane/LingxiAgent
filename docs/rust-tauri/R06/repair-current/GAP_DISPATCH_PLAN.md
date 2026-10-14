# R06 收口派发计划（GAP_DISPATCH_PLAN）

- 生成：2026-10-13，来源登记：`scripts/rust-tauri/r06_original_leaf_bindings.json`（sha256 前 16 位 `57c5107d6bd1fe16`）
- 口径：登记 279 项；READY 117 / GAP 162；其中**无任何测试绑定（未实现/未接线）84 项**，有局部后端证据 78 项。
- 本文档只是派发清单，不是验收结论；登记与回执仍以正式流程为准。

## 执行规则（提速关键）
1. **批量编辑、一次验证**：按块集中实现＋随写测试，块内全部完成后才跑一次绑定执行与回执；禁止每改一处就全量重跑（2026-10-13 当日 144 个绑定被重复执行 1210 次的教训）。
2. **共享文件单整合者**：`lingxi-service/src/lib.rs`（路由注册）、登记 JSON 与 `R06_INPUTS.json` 由一名整合者统一收口，其余执行者不改共享路径。
3. **每块完成后**：更新 `r06_original_leaf_bindings.json` 对应条目为 READY（附 proofLocations 与断言锚点），不留到最后一并补。
4. 界面类叶子（I 块及各块“页面投影”断言）统一等 GUI 观察通道解锁后批量执行；浏览器替代需 R06 评审裁定后才可用。

## 未实现 84 项按块派发

### C1-会话生命周期与授权（12 项）— 建议套件：r06_original_session_proofs.rs / r06_t03_session_tree
- `R00-T02-LA-1E0D477755CD` 从会话授权范围移除指定目录
- `R00-T02-LA-20E9C8F26CEA` 把会话分配到项目或用 null 清除归属，返回新分配
- `R00-T02-LA-21A99DB98735` 把挂载已移除或实际目录不存在的会话归档，跳过运行中会话并上报错误
- `R00-T02-LA-2D4EBE0FF3C2` 符合工作台身份的会话移到可恢复归档
- `R00-T02-LA-3818ADEAFA7A` 校验会话范围后生成可续接的新会话身份并返回路径或错误
- `R00-T02-LA-485B2D0A8A02` 轮换为新会话，旧会话归档；无历史时仍创建新会话
- `R00-T02-LA-6658E9B80314` 符合工作台身份的会话归档后永久删除文件和草稿
- `R00-T02-LA-73B631D77531` 用新目录集合整体替换原授权范围
- `R00-T02-LA-7C959E75B8C3` list返回范围；add/remove经许可后改变会话目录作用域并可再读。
- `R00-T02-LA-98DDC94776F1` 经 sessions.read 返回指定活动会话的目录授权范围
- `R00-T02-LA-9D9E72CB6051` 创建不切走当前焦点的会话；可选谱系和项目归属，深度过限拒绝
- `R00-T02-LA-FBDEEDAC0B11` 改变当前会话并尝试恢复该会话浏览器，旧会话浏览器不随切换挂起

### C2-会话历史与内容读取（10 项）— 建议套件：r06_original_session_proofs.rs / r06_t04_history
- `R00-T02-LA-3FCB259FF69C` 删除当前会话历史并提示已清除
- `R00-T02-LA-4DD3F71FA2CD` 按版本与可选替换文字重放最近消息；运行中返回 409
- `R00-T02-LA-4F1F69CCA760` 解析会话 ID 或路径，读历史并返回命中位置；Bridge 内容脱敏
- `R00-T02-LA-5DCB0352EB08` list/read返回获授权会话；send向目标会话投递；create建会话并发送首条消息。
- `R00-T02-LA-CDBFE6616EB9` 读取已授权会话的概览，未就绪返回 available false
- `R00-T02-LA-D62A81C2AEB2` 压缩旧上下文后按当前 Agent 配置重建提示词和工具快照
- `R00-T02-LA-ED089133C106` 只在已授权的会话历史内按 contentId 取大内容，找不到返回 404
- `R00-T02-LA-F0319FC4F93D` 经路径及读取授权返回滚动摘要记录
- `R00-T02-LA-F6A1A50E97E4` 读取会话消息并折叠问答卡片
- `R00-T02-LA-FF4F06011B48` 读取会话 sidecar 中冻结的系统提示词和工具名

### E-项目与文件夹目录（9 项）— 建议套件：r06_t03_session_tree（项目目录部分）
- `R00-T02-LA-059066969D44` 按所给字段更新文件夹并返回新状态
- `R00-T02-LA-3ECCF7C95483` 删除项目目录记录，原属该项目的活动会话写入未分类项目 ID；返回受影响会话路径
- `R00-T02-LA-60A58027CCCE` 按名称创建文件夹并返回新项
- `R00-T02-LA-AFA7227BFB63` 返回项目和文件夹目录
- `R00-T02-LA-D43836A3A579` 按所给字段更新项目并返回新状态
- `R00-T02-LA-D58B651BF099` 按 folderId 与 projectIds 重排并返回目录
- `R00-T02-LA-E167298FD87C` 在指定文件夹或根目录创建项目并返回新项目
- `R00-T02-LA-EEDFB7AB889C` 按 folderIds 重排并返回目录
- `R00-T02-LA-EF5DD65C558E` 删除文件夹后，原文件夹下项目移到根目录并重新排序，返回新目录；项目与会话不删除

### A-助手/配置/人格投影（15 项）— 建议套件：r06_original_persona_proofs.rs（扩展）
- `R00-T02-LA-26AE3688B8DE` 返回助手清单及有效主目录；fresh=1/true 使列表缓存失效，先清理工作区持久化残留
- `R00-T02-LA-32E9930CF9D9` 实时事件驱动情绪/人格状态展示
- `R00-T02-LA-38667593C3E1` 返回时间/会话/Agent/模型/文件/Bridge等当前值，不修改状态。
- `R00-T02-LA-38A989948FDD` 逐处读回权威目录、条件插件文件、条件模型投影、焦点 YAML、请求助手 YAML 与仅另含全局字段才变化的 preferences；失败后记录可能的部分成功，不把 HTTP 错误当回滚
- `R00-T02-LA-464E58DEC9E2` 逐处读回权威目录、条件插件文件、条件模型投影、焦点 YAML、请求助手 YAML 与仅另含全局字段才变化的 preferences；失败后记录可能的部分成功，不把 HTTP 错误当回滚
- `R00-T02-LA-517E697740D9` 助手目录与登记消失；如当前助手被删，切到替代助手并发 agent-deleted/agent-switched
- `R00-T02-LA-8B48FD65CAFB` 切换会话与工作目录，更新 last_cwd/history，发 agent-switched，返回当前上下文
- `R00-T02-LA-8D4356B7EC51` 保存顺序供助手列表显示
- `R00-T02-LA-9CC1B13F1787` 新助手持久化并广播 agent-created，返回新 id/name
- `R00-T02-LA-9E3981A9BFCA` 逐处读回权威目录、条件插件文件、条件模型投影、焦点 YAML、请求助手 YAML 与仅另含全局字段才变化的 preferences；失败后记录可能的部分成功，不把 HTTP 错误当回滚
- `R00-T02-LA-A5DE44FF584F` 返回已解析实验与有效值
- `R00-T02-LA-AD0384FBE1F2` 返回指定智能体 observation
- `R00-T02-LA-B09CC9536400` 读取已保存 MOOD 轨迹并恢复历史展示
- `R00-T02-LA-C0ADBABE5E89` 更新主助手 id，后续默认选择随之变化
- `R00-T02-LA-C99A4322D0C1` 保存已注册实验的 value 并返回生效值

### B-记忆与偏好开关（9 项）— 建议套件：r06_original_memory_skill_proofs.rs
- `R00-T02-LA-1685D5E08812` 保存enabled 等自动学习开关并返回新状态
- `R00-T02-LA-22150190C7B1` 返回当前 enabled 状态；此读取不会回退文件
- `R00-T02-LA-24BC2E961E3E` splitByScope 的全局 setter 写 preferences.json，纯全局 patch 不写 agents/{id}/config.yaml；跨助手读取同一偏好
- `R00-T02-LA-3C8CE915255E` 校验 settings.write 范围后删除当前 Agent 观察记录，返回 deleted 状态
- `R00-T02-LA-7D05B144AE21` 返回匹配经验及路径，不改记忆文件。
- `R00-T02-LA-99900EB5ACC9` 从 tenets 存储删除指定项并广播 tenets-changed
- `R00-T02-LA-B5CB2993A5C0` 返回快捷聊天窗口设置，不改动设置
- `R00-T02-LA-C3FB70F9925E` 返回enabled 等自动学习开关，不改动设置
- `R00-T02-LA-D92CD567D6D6` 保存快捷聊天窗口设置并返回新状态

### F-技能管理（7 项）— 建议套件：r06_original_skills_bundle_proofs.rs / r06_t06_skills
- `R00-T02-LA-337738D5D233` 只允许当前工作区已知技能目录；递归删除后重新同步目录，失败返回错误
- `R00-T02-LA-3516A7326DD9` 助手先删除；API 返回 skillsDeleted/skillsSkipped。当前 UI 只按 skillsDeleted 数量显示成功 toast，不显示 skillsSkipped；引用清理失败可能只记日志
- `R00-T02-LA-771296A194E0` 审核后把课程/技能文档写入Agent技能区，可在后续会话检索。
- `R00-T02-LA-7803B231FC36` 调用工具模型，返回目标语言名称映射
- `R00-T02-LA-8A69F0CFFFA1` 安装包至 .agents/skills，刷新技能路径，返回 name/installedSkillSource
- `R00-T02-LA-9CA8A47BE208` 独立技能查看器列文件并显示所选文本
- `R00-T02-LA-FAE2BDEFDB04` 构建可下载组合包及结果元数据

### G-计划与后台任务面板（6 项）— 建议套件：r03_t08_acceptance_matrix 关联套件
- `R00-T02-LA-17FCFE45576B` 仅已结束且未收纳的计划追加收纳记录，历史保留
- `R00-T02-LA-2FAA749DA790` 把命令交给同一 slash dispatcher 并以 slash_result 显示
- `R00-T02-LA-45C5CE361CCB` 有未完成项时追加完成记录并广播；已完成时返回当前面板不重复写
- `R00-T02-LA-5CD35D9EED20` 有未完成项时追加取消记录并广播，不终止其他后台任务
- `R00-T02-LA-7244E7DCE678` 返回当前会话 pending/resolved/failed 项及结果，不跨会话。
- `R00-T02-LA-B34F2B298BFA` 会话流发出完整新清单，界面显示各项pending/in_progress/blocked/completed状态。

### H-桌面壳与检查点（8 项）— 建议套件：desktop_host.rs / native_resources.rs
- `R00-T02-LA-2FB382A137FC` 返回用户选定路径供后续上传/引用
- `R00-T02-LA-3562B5D44C5E` 调用恢复并返回写回位置；检查点不存在返回 404
- `R00-T02-LA-52A5D1660088` 系统目录对话框返回路径，供技能安装/配置后续处理
- `R00-T02-LA-A2C385A33493` 悬浮窗口可见/尺寸改变；选择会话聚焦主窗并打开该会话
- `R00-T02-LA-BB0A168006D7` 只接受绝对文件路径和 edit-start/autosave-interval 原因，返回新检查点
- `R00-T02-LA-BDE3AC44D79C` 只允许 cut/copy/paste/selectAll，并向当前 webContents 派发该命令
- `R00-T02-LA-CAA876503B1B` 本地普通文件交由 OS 默认程序打开
- `R00-T02-LA-D901852E1B1A` 按 sessionId 保存；sessionPath 先解析为 sessionId

### I-卡片/地图/界面组件（8 项）— 建议套件：r06_t05_preview / r03_t08 界面关联套件
- `R00-T02-LA-5271217DB01B` 未指定坐标被删除
- `R00-T02-LA-5370DA3742CB` 仅指定卡片变化
- `R00-T02-LA-8E2D44B48905` 逐动作核对实际状态与页面投影；失败不伪装成空数据
- `R00-T02-LA-94B9BE3AA7A6` 折叠列表整体更新
- `R00-T02-LA-94CB746D04B7` 输入区出现一张批量提问卡；确认返回按key映射的答案与answered=true；超时只自动填推荐项并标timedOut/autoSelected；拒答返回dismissed且answers为空。
- `R00-T02-LA-D51B2A1FA68A` 返回指南文本，供后续show_card/文件编辑使用；本调用不创建卡片。
- `R00-T02-LA-EE2ACB390FCE` 生成cardId并把title/code存入会话结果，前端在隔离iframe渲染卡片。
- `R00-T02-LA-F28086B64522` 返回持久化布局或默认空布局

## 已有局部后端证据、待界面/端到端收口的 78 项
这些项后端已实现且有部分测试，主要差“页面投影/广播/端到端”证明；按既有套件归位即可，不需要重新开发：
- r06_t05_resourceio：25 项
- r06_t03_session_tree：20 项
- r06_original_memory_skill_proofs：19 项
- agent_memory_r06t06：17 项
- r03_t08_acceptance_matrix：17 项
- r06_original_knowledge_proofs：15 项
- r04_t04_file_tools：15 项
- r06_original_skills_bundle_proofs：14 项
- r06_t06_memory：12 项
- r06_ar_f04_lifecycle：12 项
- r06_t05_filehistory：9 项
- r06_original_resource_proofs：8 项
- r06_t02_compaction：7 项
- r06_t04_history：6 项
- r06_t06_skills：5 项
- auth_matrix：5 项
- r06_t05_fsread：4 项
- cancel_link_inheritance：4 项
- r06_original_session_proofs：4 项
- event_subscription：4 项
- subagent_permission_inheritance：3 项
- r06_t05_preview：2 项
- r06_t05_session_files：2 项
- background_steering：2 项
- lib：2 项
- r06_t07_knowledge：1 项
- r04_t03_approval_service：1 项
- r06_t06_skill_context：1 项

## 收口顺序
1. 用户侧先决：GUI 弹窗人工点选通道（已验证可行，重跑即出证据）；独立审查平台中断恢复；缺失的 2026-10-08 原始执行提示词由用户补交或裁决。
2. C1→C2→E→A→B→F→G 并行派发（H/I 及界面断言随 GUI 通道解锁收口）。
3. 全部 READY 后：冻结候选 → 单次全量绑定执行 → R06 正式门禁 + R05→R04→R03→R02 回归链 → 全新 Reviewer 独立签收（覆盖 14 项开放 AR 发现）。

## 边界
- 不新增无关优化；真实付费模型、跨平台发布维持原授权边界不提前。
- 本计划不改变 R06_NOT_ACCEPTED 状态；`R07_READY` 只由正式门禁与独立验收改变。
