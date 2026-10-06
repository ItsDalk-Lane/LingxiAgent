# B-R2 R2 轮 3 微修复验证记录（b-r3-note-fix）

- 轮次：R05 RR2 WP-B 第 3 轮微修复（note 反引号命令替换缺陷）
- 对象：scripts/rust-tauri/r02_t08_legacy_entry_regression.sh 第 3177 行 `note "PASS E5-cause-classification (…)"` 中
  B-R2 登记描述里的未转义反引号 `` `error: patch too large` ``（双引号内被 bash 当命令替换执行）。
- 改动：仅该一处，`` `error: patch too large` `` → `` \`error: patch too large\` ``（转义反引号，note 打印文本逐字保留含反引号原貌）。
  同段扫描（grep -n '`' 全文件）确认 3177 是 B-R2 新增段内唯一非注释行反引号缺陷；
  2595 行 `fail "E0s: …bare \`Test Files 1\`…"` 为既有 E0s 段内容且不属 B-R2 新增段，按任务范围未动。
  逻辑、字符串语义、其他内容零改动；未 commit/push。

## 验证 1：bash -n

命令：
```sh
bash -n scripts/rust-tauri/r02_t08_legacy_entry_regression.sh && echo "bash -n OK"
```
输出：
```
bash -n OK
```

## 验证 2：提取该 note 实际执行（stub note()，分离 stdout/stderr）

replay-harness.sh 只重放 classify_file 路径，到不了 E5 note；按任务书允许的等价最简方式
直接提取第 3177 行并执行。命令：

```sh
bash -c 'note(){ echo "$@"; }; eval "$(sed -n "3177p" scripts/rust-tauri/r02_t08_legacy_entry_regression.sh)"' \
  >/tmp/b-r3-note-stdout.txt 2>/tmp/b-r3-note-stderr.txt
rc=$?   # = 0
```

输出/断言：
- exit=0；stdout 1556 字节（note-stdout.txt），stderr 0 字节（note-stderr.txt）。
- `grep -o "complete \`error: patch too large\` refusal"` 命中：
  ``complete `error: patch too large` refusal``（完整字样含反引号原样打印）。
- `grep -c 'error: patch too large' note-stdout.txt` = 1。
- `grep -c 'command not found' note-stderr.txt` = 0。

## 对照：修复前形态复现（同提取法，把 \` 还原为 ` 模拟旧稿）

命令：
```sh
bash -c 'note(){ echo "$@"; }; eval "$(sed -n "3177p" … | sed "s/\\\`/\`/g")"' \
  >/tmp/b-r3-note-old-stdout.txt 2>/tmp/b-r3-note-old-stderr.txt
```
输出（prefix-sim-stdout.txt / prefix-sim-stderr.txt）：
- stderr 出现 ``bash: error:: command not found``，stdout 中该片段被替换为空
  （`grep -c 'error: patch too large'` = 0，1532 字节 < 修复后 1556 字节）——
  与缺陷描述一致，证明修复命中根因。

## 结论

- bash -n 通过；note 打印包含完整 `error: patch too large` 字样，无 "command not found"。
- 未重跑完整 r02_t08（阶段终审会跑）。
- 附件：note-stdout.txt / note-stderr.txt（修复后），prefix-sim-stdout.txt / prefix-sim-stderr.txt（修复前对照）。
