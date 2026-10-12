# W05-E01 材料与生命周期取证

本 Task 只修缓存身份、下一 Run 生效、实际调用快照与有界回收。原段模板、段注册、排序、预算、两层权限、工具配对/代次规则不改。Session owner、run actor、授权 reader 分开；LocalUser 原广域访问保留，Device/Web 原本人范围保留。HTTP 不因此新增 arbitrary Agent 入口；合法不同 Agent child 仍从真实 parent/Runtime 派发。

| 材料/坐标 | 当前真实读取源 | 实际写入入口/变化入口 | 生效与回收 |
|---|---|---|---|
| identity/AGENTS/public persona | FileContextMaterialSource + resolve_persona_source；AGENTS.public 的原裁剪合同不扩充 | persona PUT 与 ishiki/public-ishiki aliases → PersonaFiles.write_persona/write_public_persona → atomic_write；合法直接文件替换 | 新 Run 重读；本 Run Arc 不改；原模板/语言回落保留 |
| yuan/type/name/locale | 当前 Agent config.yaml 窄扫描；product yuan/templates；Agent locale → preferences locale → en | memory config PATCH → patch_agent_config；Skills whitelist 同文件行级 patch；其他 Agent config 改名/配置影响 roster；合法直接文件更新 | 新 Run 重读并确定性校验；本 Run 冻结。完整 YAML 缺口属于仍 OPEN 的 F06，不声称修复 |
| username/profile/experiments/learn_skills preferences | user/preferences.json 同次真实 JSON；user/user.md | ContextNotesStore.write → atomic_write；Skills 外部来源/隐藏状态 preferences 写；现役可信直接文件写入 | 每次采集 fresh，无永久 prefs/hidden/external 源缓存；读取/JSON 错误响亮，失败不沿用旧 artifact |
| compiled memory / active tenets | Agent memory/memory.md 与 tenets.json | CompiledMemoryStore facts/today/longterm/week/write_week_day/clear/migrate → atomic_write/remove；TenetsStore 写；DreamRevisionStore.restore 整体 guard | 服务多文件发布有外层 guard + 原共享 atomic_write guard；新 Run 生效。本 Run material 与摘要不变 |
| facts/experience DB、search/learn/dream hooks | 原 T06 stores / actual operation ports；编译源只消费当前已编译 memory.md/tenets | 原 DB/MemoryTools/Dream 修订入口 | 不伪造事实写立即生成不存在的 compiled source；原 T06 成功/失败全合同仍 REQUIRED/OPEN，未新增无调用的失效接口 |
| runtime Skills | SkillMaterialPort → SkillService.runtime_skill_catalog；本次 preferences 单一文档；Agent whitelist/workspace_disabled/policy；pool/builtin/external/workspace 同源选择 | 手工 install；Agent install 的审核/确认后实际发布；启停/批量 whitelist；bundle apply；delete/uninstall/hide；external-path PUT；reload；合法直接 SKILL.md/preferences/config 更新 | 安装/启停/删除/外部源/reload 同 home publication epoch；新 Run fresh catalog；current Run 的 skill.catalog 属 system 材料冻结。原 install/风险确认/默认不启用/工作区遮蔽/执行权限继续独立 |
| bundles 元数据、头像文件 | 管理源；不是当前 ContextCompileInput 直接消费的正文 | 原 bundles CRUD/order；avatar upload/delete | bundle apply 实际 whitelist 写已覆盖；头像/完整 vision/persona 同步原未实现义务保持 OPEN，不能借缓存申报完成 |
| appearance summary/roster | appearance-summary.json；agents/*/config.yaml | 实际现役可信文件更新；当前服务无额外 appearance-summary 生成入口 | 新 Run 重读；原缺省与原未接通能力不扩展 |
| Session coordinates / memory | 权威 session_tree 行 created_at、owner、memory_enabled；真实 Run agent/child shape/actor | create/fork/retry/rewind 原权威入口；Session memory PATCH（原允许修改时机保留） | created_at/owner/agent/shape/actor 固定进本 Run；Session memory 仅影响本 Session 本次新 Run，不篡改 Agent master 开关 |
| host/product/cwd | bootstrap HostFacts / product_dir 原解析 | 服务启动实际环境/config workspace；product 文件实际部署/直接更新 | 主机事实按原 bootstrap 合同；模板文件新 Run fresh；未发明尚无生产入口的动态 workspace/config 完整支持 |
| live tools | 原 ToolDeclarationSnapshot::from_registry 每个 model call 重建；CURRENT state at send time | 原 registry update/availability/uninstall/MCP catalog generation | system/material 每 Run 固定；真实 call 的 tools/generation/完整声明各自持久观测，旧 call 不变；prepare/execute 继续实时权限复核；新发现禁用后的历史编码缺口 OPEN |
| actual observations | 同一 Arc artifact + 同一实际 turn_input.tools；原 durable key_events schema lingxi.model-context-snapshot.v1，关联 model_call_started/run/attempt | 真实 driver 调用开始写入；HTTP 查询按可选 runId/modelCallId/snapshotId；查询不重编译 | DB hash/owner/actor/session/run 一致性校验；不存在 404；无迁移/no lock change；失败本地 call 的 driver attempt 与物理 wire 必须区分（新 OPEN finding） |
| cache/lifecycle | 已提交调用观测的有界索引；材料不全局缓存 | record_call_observation；SessionAdmin archive/descendants/永久 delete/cleanup；服务退出 | 128 entries 且 8 MiB，FIFO，evictions/reclaimed/entries/bytes 可观察；当前 Run Arc 不被淘汰影响；淘汰/归档/重启历史持久回读，删除 404；弱 home epoch 索引不保存历史资料 |

服务写入先进入同 home 活动发布栅栏。实际采集两遍对照内容并比较发布代次；并发/嵌套写者全计数，活动发布或变动不能提交混合采集。最多八次立即有限重试，持续冲突响亮失败，无 TTL 或旧版本假成功。直接合法文件替换按原 fresh 文件读取合同进行双采集对照；没有发明外部多个独立文件替换的跨文件事务承诺。

Kernel 唯一改动为输入/子项 PartialEq，原段模板、公式、预算、裁剪和渲染全部原样。未碰 migrations/registry/stage_maps/permission handlers/codec assertions；新增 key_events 使用原 unknown-event 扩展合同。
