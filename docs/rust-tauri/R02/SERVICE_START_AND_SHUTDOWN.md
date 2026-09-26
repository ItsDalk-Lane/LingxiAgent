# lingxi-service 最小启动说明与关闭失败诊断手册（R02-T08）

> 交接 R03 的操作面文档。描述的是 **Rust 新栈独立服务**（`lingxi-service`），
> 不是现役 Electron/Node server 的启动方式；R02 未替换任何生产默认入口
> （R02-A16）。所有路径以 `{home}` 表示显式给出的服务数据根（合成/测试
> 环境一律用 /tmp 新建目录，绝不指向真实用户目录——数据根优先级见下）。

## 1. 构建（锁定工具链，离线）

```bash
# 经 rustup 锁定 1.98.1（rust-toolchain.toml），--locked，离线，专属 target
env -u http_proxy -u https_proxy -u HTTP_PROXY -u HTTPS_PROXY \
    -u all_proxy -u ALL_PROXY CARGO_NET_OFFLINE=true \
    CARGO_TARGET_DIR=/tmp/rust-target-manual \
    rustup run 1.98.1 cargo build --manifest-path rust/Cargo.toml \
    --locked -p lingxi-service
# 二进制：$CARGO_TARGET_DIR/debug/lingxi-service
```

## 2. 最小启动（一个进程，无需桌面）

```bash
/home=/tmp/lingxi-manual-home   # 绝对路径；不存在则创建
mkdir -p $home
$BIN --home $home
# stdout 恰好一行机器可读就绪行：
#   LINGXI_SERVICE_READY addr=127.0.0.1:<port> home=/tmp/... source=cli
# 其余诊断走 stderr（经脱敏；进入 {home}/lingxi-service/logs/ 轮转文件）。
```

要点（详细契约见 `rust/crates/lingxi-service/src/main.rs` USAGE，`--help` 可打印）：

- **数据根优先级**：`--test-mode` > `--home` > `LINGXI_HOME` 环境变量 >
  `--config` 文件内的 `home`；无任何来源 = 拒启（exit 2）。没有"默认落到
  真实用户目录"的路径。
- **网络**：默认 `127.0.0.1:0`（loopback + 临时端口）。非 loopback 绑定必须
  显式 `--network-mode lan`；loopback 也**必须认证**（无免认证豁免）。
- **认证**：启动时在 `{home}/lingxi-service/local-token.json` 写入
  per-start 环回 token（0600）。HTTP/WS 请求带
  `Authorization: Bearer <token>`。重启后 token 更换，旧 token 失效。
- **单写者**：同一 `{home}` 第二实例拒启（exit 3 +
  `LINGXI_SERVICE_SINGLE_WRITER_BLOCKED` 标记行）；锁（OS 文件锁）是唯一
  存活权威，陈旧记录自动归档为 `instance.stale.json` 并接管。
- **停止**：向进程发 SIGTERM（或 Ctrl-C）；graceful 关闭链 = 停收新请求 →
  广播 WS 会话关闭（1001）→ 等待/取消受管任务 → flush 关键事件 → 关闭
  run 数据库（WAL TRUNCATE checkpoint）→ 移除自己的 instance 记录 → 释放锁。
  正常退出码 0。
- **首个业务调用**：`GET /lingxi/v1/health`（无需 token，最小面）；读会话
  `GET /lingxi/v1/sessions/{id}`；写 `POST /lingxi/v1/sessions/{id}/execute`
  （`{"input": "..."}`）；订阅 `GET /lingxi/v1/ws` 升级后发
  `{"type":"subscribe_events","streamId":"<session>"}`。
  内置种子会话：`sess_local_alpha`、`sess_local_beta`。

## 3. 数据布局（新栈，全部在 `{home}` 下；与旧 Node 数据根分离，ADR-004）

| 路径 | 内容 | 性质 |
|---|---|---|
| `{home}/data-epoch.json` | data-epoch 印章（闸门） | epoch 管理 |
| `{home}/lingxi-service/instance.lock` | 单写者锁（OS 文件锁权威） | 实例身份 |
| `{home}/lingxi-service/instance.json` | 实例记录（原子写） | 实例身份 |
| `{home}/lingxi-service/instance.stale.json` | 被接管陈旧记录的归档 | 诊断 |
| `{home}/lingxi-service/local-token.json` | per-start 环回 token（0600） | 凭证 |
| `{home}/lingxi-service/data/runs.db`(+`-wal`/`-shm`) | 新 run/消息/关键事件 SQLite 库 | 权威业务数据 |
| `{home}/lingxi-service/logs/service-*.log` | 脱敏轮转日志（0600，默认 5MiB×7） | 诊断（可重建） |
| `{home}/lingxi-service/tmp/` | 原子改名暂存目录 | 内部 |

## 4. 退出码表（与 USAGE 一致）

| 码 | 含义 |
|---|---|
| 0 | 干净停止 |
| 1 | serve 失败（运行期错误） |
| 2 | 配置/启动错误（含数据 epoch 闸拒绝：`LINGXI_DATA_EPOCH_*` 标记行） |
| 3 | 单写者锁被对端持有（或锁 IO 失败） |
| 4 | 关闭期实例记录清理失败 |
| 5 | 关闭期 run 数据库 drain/checkpoint 失败 |
| 6 | 某关闭阶段超过 `--shutdown-timeout-ms`（显式记录，关闭继续） |

## 5. 关闭失败诊断手册

先取证据：stderr 全文 + `{home}/lingxi-service/logs/` 最新轮转文件 +
`LINGXI_*` 机器标记行。所有关闭期失败都有显式标记行，不存在静默吞错。

### 5.1 关闭挂住 / 卡在某阶段（SIGTERM 后进程不退）

1. `ps -o pid,stat,command -p <PID>`：状态是否 `U`/`E`（不可中断/退出中）。
2. 看日志尾部是否出现 `LINGXI_SERVICE_SHUTDOWN_TIMEOUT` 标记行——出现说明
   某阶段超过 `--shutdown-timeout-ms`（默认 10000ms），关闭**仍在继续**，
   进程最终以退出码 6 结束。这是显式超时，不是死锁。
3. 疑似受管 WS 会话未退（关闭广播已发但对端不消费）：核对日志中
   `ws session close` / 队列统计行；用 `lsof -p <PID>` 查看遗留连接对端。
4. 疑似 DB 关闭卡住（阶段 3）：`lsof -p <PID> | grep runs.db` 确认还有哪些
   打开句柄；外部读者（如 `sqlite3` 只读探针、`lingxi-storage-inspect`）
   持 WAL 读锁可能拖慢 checkpoint——等外部读者退出后重试关闭。
5. 强杀是**最后手段**且必须记录（`kill -9` 会跳过记录清理）：强杀后重启会
   看到"锁空闲但记录残留"→ 陈旧记录归档 + 接管（这不是错误，日志有
   `LINGXI_SERVICE_STALE_RECORD_TAKEN_OVER`）。

### 5.2 退出码 4：实例记录清理失败

stderr：`error: shutdown instance-record cleanup failed: <原因>`。
含义：数据库已关闭，但 `instance.json` 未删成（IO/权限）。对端会把它当
陈旧记录（锁已释放）——语义安全，但应修权限/磁盘后重启一次让它归档接管。

### 5.3 退出码 5：run 数据库关闭失败（drain/checkpoint）

stderr：`error: run database shutdown failed: <原因>`。WAL 未 checkpoint
不丢已提交事务（SQLite 恢复语义），但必须先处理根因再启动：
- `SQLITE_BUSY`：外部进程（旧探针/备份工具）持有库句柄 → 释放后重启；
- `disk full` / IO：清理磁盘或修挂载后重启；
- `corrupted` / `schema_tampered` / `database_too_new`：epoch/完整性闸会
  在下次启动**拒启**（exit 2）而不是清库——按 5.4 处理。
验证修复：`lingxi-storage-inspect backup --source <runs.db> ...` 或只读
`sqlite3 ... "PRAGMA integrity_check;"` 确认库完好。

### 5.4 重启被 epoch/完整性闸拒绝（exit 2）

stderr 会有 `LINGXI_DATA_EPOCH_BLOCKED` /
`LINGXI_DATA_EPOCH_TRANSITION_INCOMPLETE` 标记行 + 双语诊断。语义：
**fail-closed，绝不自动清空/降级库**。处置顺序：
1. 只读确认印章与库完好（integrity_check；对照 `{home}/data-epoch.json`）；
2. 修复外部根因（半完成的迁移、被截断的印章文件等）；
3. 数据确实不可恢复时，先做**完整备份副本**，再走显式人工决策——
   不存在"删掉重来"的自动路径。

### 5.5 第二实例误启动（exit 3）

stderr 首行 `LINGXI_SERVICE_SINGLE_WRITER_BLOCKED home=... recordedPid=...
probe=live|unreachable ...`。`probe=live`：对端真活着，去操作对端；
`probe=unreachable`：记录残留但锁已释放 → 本次启动已自动归档接管
（继续用新实例即可）；若反复出现且无对端进程，检查是否有跨主机的
NFS/网络盘共享 home（文件锁不跨主机可靠——部署约束，R09 再收口）。

### 5.6 日志文件写入失败（`LINGXI_SERVICE_LOG_WRITE_FAILED`）

含义：轮转文件附接失败，服务**显式降级为 stderr-only**继续服务（不拒启、
不静默）。修 `{home}/lingxi-service/logs/` 的权限/磁盘后重启恢复双写。
`--log-max-bytes`（最小 64）/`--log-max-files`（最小 2）低于下限是解析期
exit 2，不会走到这里。
