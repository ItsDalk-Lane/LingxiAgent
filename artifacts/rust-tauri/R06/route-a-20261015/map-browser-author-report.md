# Map与焦点浏览器恢复作者交付

作者最终PASS，申请独立审阅FBDEED与8E2D44，不自行登记。补丁26源：`/private/tmp/r06-session-completion-delivery/map-browser-final.patch`，SHA `ff4a3e502d1fac6879d13fd88782506e6344bab0d5e9d8119726de530881e080`。精确base `fbb0c1041c9535100ff41274e8c1e6b66da105e7cc99a09ee7b518f992eb3336`、candidate `dd7a2cdaaedcf270a9fbab378c066b7e68e0f996f03d33159fa7025cf577b485`，逐文件及冻结副本见 `map-browser-final-source-sha.json` / `map-browser-final-candidate/`。精确base上apply --check通过，最后检查前后26源SHA一致。

## 结果与原义务

1. Map原第1组：实际主体布局及canonical turns/连边；真实外部焦点展开collapsed并写盘；另一真服务空图同步清旧状态，旧DOM同帧操作零旧SID/布局越源写入。真实目录占用布局文件使HTTP500，console.warn、loaded=false且原字节备份不变。
2. 第2组：工作区/卡片焦点及文件/模型刷新、无cwd零文件GET；同当前卡零switch/history；同权威修订缓存零GET、外部真实HTTP新回合后切回恢复新历史。归档目标拒绝时当前会话未切，但mapSelectedCard已变且出现错误，未称视图完全不变。
3. 第3—6组：真实鼠标拖动/相机缩放定位、positions合并/替换、collapsed替换并重载；NaN/Infinity本地零PUT。真实uchg分别验证拖动/折叠/重置失败：本地乐观结果保留，旧磁盘不变，重新加载以磁盘为准。16并发不同位置写全部保留。
4. 第7组：生产加号/详情面板draft来源、空白零提交、缺锚点零fork、实际uchg fork失败保留原文。continue原SID真实prompt且无fork/切换，branch实际fork→新SID prompt/焦点/待答卡。真供应商进行中第二条进原队列，零额外prompt。SIGSTOP仅合成服务，实际WS发出、原15秒账本delivery_unknown后保留原文并toast；恢复后实际accepted/terminal再点只关闭原draft，零重复。未改协调器时限/判定。
5. fork保持唯一SQL事务，memory/project/权限与授权根继承；metadata与唯一chat choice在新SID admission内准备，真实失败撤回自身预备资料。旧未显式选择会话在准备前冻结实际gateway pair/思考，下一真Run验证消费。
6. FBDE：新增LocalOnly本机browser-host WS，复用认证/升级/连接上限/关停，16有界队列及相关ID/5秒回执；setupBrowserCommands的Rust分支接真实Bearer宿主，仍调用生产handleBrowserCommand resume。真实Electron视图B挂回，A页面JS仍可读且视图存活，不弹窗。无目标浏览器真实attempted/found=false；外来宿主403、重复宿主409；缺会话404/越权403零偏好副作用；真实TCP暂停/断开502，自动重连后恢复。断回执时焦点可能先提交、视图可能已恢复，不承诺整体回滚。

## 最终验证

Rust四套共 **20 PASS**：旧r06_conversation_map_and_checkpoint 3、r06_session_list_projection 1、新r06_session_map_browser_completion 2、旧r06_t03_session_tree 14。此前“21联合”口头数字是加法误差，以最终20日志为准。新Map父含19真窗口检查、原七组齐。36供应商HTTP请求/12真实User Run，每Run3调用，36 reported账本，均20输入/4输出token成功；default/off6、second/high30；4 UI prompt。费用逐Run/逐模型检查，队列撤销和已收旧输入零多付费。

原地图Vitest25逐字未改PASS；三TS/strict Clippy/fmt均PASS。当前父从CARGO_BIN_EXE启动真实CLI，不引用旧测试exe。Rust完整命令：

```text
CARGO_TARGET_DIR=/private/tmp/lingxi-r06-agent-delete-target CARGO_INCREMENTAL=0 LINGXI_HOME=/private/tmp/lingxi-r06-agent-delete-env/home TMPDIR=/private/tmp/lingxi-r06-agent-delete-env/tmp R06_NODE_BIN=/Users/study_superior/.nvm/versions/node/v24.16.0/bin/node /Users/study_superior/.cargo/bin/cargo test --locked -p lingxi-service --test r06_session_map_browser_completion --test r06_t03_session_tree --test r06_conversation_map_and_checkpoint --test r06_session_list_projection -- --test-threads=1 --show-output
```

Map原始窗口：`/private/tmp/lingxi-r06-agent-delete-env/tmp/lingxi-r06-map-62620-1792039491195670000`，result.json、map-ui-result.json、raw-provider-requests.json、cached-history-after-external.json及map-window.png；截图已实际查看，连边与当前会话强调可见。Browser原始窗口：`/private/tmp/lingxi-r06-agent-delete-env/tmp/lingxi-r06-browser-62620-1792039483121338000`，browser-result.json/窗口日志及home真实偏好。Browser腿从当前main.cjs AST精确提取生产接线与恢复函数，用真实BrowserWindow/WebContentsView执行，没有替换恢复核心或网络回执；不冒称完整应用冷启动。

原FAIL均保留：unknown丢草稿、旧fork model/thinking=null、同修订额外GET、旧CLI browser-host404，精确路径/原事实见JSON。此前“缓存会陈旧”为静态推断，实际旧reconcile会额外GET，未实证陈旧，撤回该claim。原坏JSON200默认是旧兼容义务，不能以200本身叫产品FAIL；作者strict500导致旧断言失败的日志也保留，最终保持200默认+degraded/readError，页面warn并保留旧图，写坏原文拒绝且字节不动；真正I/O错误500。CDP准备/错误夹具路径/不可见拖动坐标/类型或编译诊断不是产品负控。

## 整合与限制

保留Root V14 getter早于PreparedForkUi、唯一fork INSERT proactive列/末参数；gateway是独立builder/准备末参数。main.cjs只setupBrowserCommands块；session-actions只原生list修订规范化及switch缓存块，保留81B fetch函数；lib/auth/sessions按精确hunk保留Main Read/Mood/资源。session_created renderer 7文件另包，不重复纳26，81B已含白名单token。

仅macOS现役Electron组件/真实宿主函数与Rust网络；非Tauri/Win/Linux窗口，不声称完整main.cjs应用冷启动；本机HTTP供应商替身，非外部付费供应商。工作区覆盖实际绝对cwd及无cwd；未登记mountId明确400，不声称已注册挂载链。无宿主明确skipped/no_browser_host；有宿主但无浏览器实际attempted。未改Main/登记/门禁/白名单/原78断言，未提交推送。完整JSON列所有检查、命令、日志与SHA。
