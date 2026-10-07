# DELIVERY-PREP-01 交付范围快照

先读 [REPORT.md](REPORT.md)。这是只读前置清点，未暂存、提交、推送，不是最终冻结或远端可达证明。

- 最终分类：[paths-resolved.json](paths-resolved.json)；初步路径列表仅供复审：[include-paths.txt](include-paths.txt)、[local-paths.txt](local-paths.txt)、[review-paths.txt](review-paths.txt)。
- 生产及永久回归：[production-scope.json](production-scope.json)。
- 引用归属及实际存在：[references-resolved.json](references-resolved.json)。初步 `references.json` 含未做相对根语义校正的条目，不作最终缺失判断。
- 本地原件：[local-originals.json](local-originals.json)。这些条目的 `localOnly=true`；原件本地保留，本轮没有远端交付或验证远端归档。条目给实际内容SHA、大小及历史引用来源。此索引不代替原件。
- 实際原件与历史SHA：[local-original-reference-hash-check.json](local-original-reference-hash-check.json)。符号链接文本与外部程序内容不是同一哈希对象。
- 真命令与时间/退出/输出hash：[commands.json](commands.json)、[check-ignore-command.json](check-ignore-command.json)、[finish-command.json](finish-command.json)。未执行历史命令里的测试或构建。

当前忽略规则会忽略通用 `target/`、`node_modules`，会重新包含RR3日志；但本文列出的大rlib、已留存程序和测试home令牌都没有相应ignore保护。因此 `git ls-files --others --exclude-standard` 列出来不等于可安全直接提交。按文件用途和来源精确复审，禁止整目录暂存。

后续J、G、E与FINAL完成并停止写入后，须另建最终清点目录刷新相同规则；不能把当前列表当最终候选名单。
