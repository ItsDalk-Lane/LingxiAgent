技能七叶交付（私有候选，未提交、未推送、未登记）

基线为 c3989a6c9a8983d24b93e95e8faf832ca525affb。源码目录为 /tmp/lingxi-skills-completion-20261015-r1。skills-seven.patch 是基于该提交的21文件净补丁；skills-seven-manifest.json 列出每个文件的基线/候选SHA256、新增标识、候选内容集合SHA256与补丁SHA256。候选集合SHA不是Git提交SHA。已在Main最新工作区只读执行 git apply --check，退出0。

实现：
- 保留现有技能服务结构，增加目录/zip/.skill/base64完整包安装；全局安装不自动启用，面板指定助手安装会启用。工作区安装写入所选且获准的 .agents/skills，返回真实 installedSkillSource。拒绝跨工作区、链接、越界压缩条目及无写权限，错误不伪装成功。
- learn_lesson.learn 接入现有工具注册和助手上下文，使用现有安全审核及与正文绑定的一次性风险令牌；先审核后写入，启用失败撤销新课程目录，真实经验索引与运行时目录读回可见。
- 名称翻译走现役 AuxiliaryExecutor 的 Title 槽（与现役TS引擎选槽一致）、Model quota和模型调用账本；只提交当前助手可见名称，解析完整返回映射，供应商/解析失败明确返回错误。
- 补全目标全局偏好接口，沿用已有自动学习偏好与全局配置写入；补全设置/面板已迁移技能路径。外部目录保存后重载失败可以到达失败提示。
- 增加真正独立Tauri技能窗口、只读树与文本；根路径绑定窗口，拒绝其他窗口根和写命令。非绝对路径/非目录为空树，非文本/大于2MiB为空正文。查看从不安装。
- 重载仍使用既有安装/删除共享锁，扫描后广播既有 skills-changed；前端按助手重读。未把基线已存在广播标为未实现。

原断言与证据映射：
- 0DDB5AA05072、99F8CD228733：service父测试 original_skills_settings_and_panel_current_real_service_projection 启动真实RustHTTP服务，再运行现役 SkillsTab、SkillsPanel 与四个原界面测试文件；新增live用例只替代原生选择/打开/显示文件边界，其HTTP、权限拒绝、包安装、组合CRUD/排序/成员/启停、导出文件、偏好写入、事件与最终磁盘读回均真实。skills-ui-evidence/receipt.json 有15项完成断言及逐请求状态，stdout.log 为5文件33/33。原32项保持原断言，包含冷启动/缓存/15秒/初始化异常及base64界面分支；这些原测试使用其原有隔离依赖，不能说全部33项都是真实供应商/真实原生IPC。
- 771296A194E0：original_lesson_review_one_shot_content_binding_conflict_and_real_retrieval；未接受风险无写入、改变正文作废令牌、重新明确接受后实写、冲突/缺助手/坏名称/写入根错误拒绝，技能正文、助手A/B配置、ExperienceStore正文/索引、运行时列表均读回。审核测试使用明确的可控SkillGuardPort，真实外部审核供应商没有调用。
- 7803B231FC36：original_translation_uses_real_title_model_protocol_accounts_and_refuses_cross_catalog；本地真实OpenAI SSE协议供应商经生产ProviderRegistry/ModelGateway/AuxiliaryExecutor，校验映射/失败/越权/空参数/en/实际用量账本。外部付费供应商未运行。
- 8A69F0CFFFA1：original_skill_packages_workspace_source_global_and_agent_install_rejections；工作区完整配套包/安装来源/刷新及不获准路径、只读凭证、坏包、缺助手拒绝。
- 9CA8A47BE208：original_native_skill_viewer_current_tauri_window_and_read_boundaries。父测试构建当前原生前端并启动当前Tauri测试，真实两个窗口/默认正文/点击文件树/超限空正文/独立根及写命令拒绝/原文件字节不变/不安装均有硬断言。最后一次本轮GUI证据在 /var/folders/5r/wq54gtms21gfjgr5v94ynfyh0000gn/T/lingxi-skills-native-proof-0ca608f5bb2467af，receipt.json有11项，skill-viewer.png已目视核对所选中文文件原文。仅macOS本机，Windows/Linux NOT_RUN。
- BA490D930CA9：真实重载HTTP与事件随后投递现役界面检查按助手GET；新增单元 original_reload_install_delete_wait_for_the_same_mutation_lock 持有生产共享锁，三个独立线程分别执行reload/install/delete，等待开始后均不能越过该锁，释放后均结束；不只是检查源码字符串。

原始失败：baseline-probe/command.json 和 result.json 绑定原基线；baseline-original-failure.log 为真实HTTP base64安装返回400“path must be an absolute skill package path”，期望200而退出101。原基线源码在私有目录实际恢复执行后已恢复候选。panel-actions-r2.log另保留本轮发现Panel技能/组PATCH路由遗漏的实际失败；修复后panel-actions-r4.log真实状态通过，没有放宽断言。

验证记录：
- completion-delivery-final.log：完整5/5父测试通过，包括真实当前Tauri窗口；该次之后仅增加原32界面用例到UI父命令，另追加cfg(test)共享锁证明，生产原生源码未变。
- completion-ui-delivery-final.log：最终UI父测试通过（内部33/33）；skills-ui-evidence为此最终收据。
- shared-lock-delivery-final.log：最终共享锁证明通过。
- regression-final.log：23/23（已有事件1、已有完整包/组合12、新父非native4、已有T06技能6）。additional-affected-regression.log：上下文4+全局偏好2均通过。
- clippy-delivery-final.log：服务lib及新父测试严格Clippy；native-clippy-final.log：当前native lib+skill_viewer_host严格Clippy；native-tscheck.log：原生前端定向TypeScript检查。

复现：现有service缓存 CARGO_TARGET_DIR=/private/tmp/lingxi-ex01-target，CARGO_INCREMENTAL=0，cargo绝对路径 /Users/study_superior/.cargo/bin/cargo +1.98.1，进入私有rust目录使用 --locked。新父命令 test -p lingxi-service --test r06_skills_completion -- --nocapture。共享锁命令 test -p lingxi-service --lib original_reload_install_delete_wait_for_the_same_mutation_lock -- --nocapture。native父使用已保留 /Users/study_superior/.cache/lingxi-r06-builds/tauri-resources，不复制或新建target。

合并注意：App.tsx只有技能入口小hunk，保留Root资源改动；bridge/native lib/build注册同样只增技能。use-hana-fetch.ts新增精确skills及agents/.../(skills|skill-bundles)映射，需与聊天作者更大路由映射合并，不能用私有旧文件整份替换。服务lib/auth也是小注册hunk。门禁、原断言、白名单、Main及用户真实资料未修改。是否正式登记七叶由Root最后当前合并候选与门禁验收决定。
