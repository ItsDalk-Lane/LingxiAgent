# 候选校验有界并行的只读审阅

结论：最终外置实现静态 scope PASS。这是同一完整校验的计算方式提速，没有发现判定、白名单、文件范围、checkpoint次数或错误拒绝的弱化。未改 Main，未构建或执行测试，不影响现行正式进程。

- candidate_final.rs SHA256：`75dbb279a83b6a42356b0c44f60407c7a275448da46d959561da2f1f92c23f3a`
- tests_final.rs SHA256：`b18ba55301f7a36f1d53d2320ed7a1b2d7dbcec5de49ebe4ee8d9d87535c7b4e`

实现保留git全量枚举、原字节排序/去重和排除检查；每文件仍执行原hash_candidate_file，原安全open/no-follow/前后文件身份/父目录身份/模式及字节读取未改。由reject_tracked_output起的所有原辅助函数尾部逐字相同，最终聚合体逐字相同。最多4线程处理连续块，显式join全部成功创建的句柄后，再按原块顺序处理Result与Entry；spawn失败、panic、任何文件Err均拒绝，没有filter_map、缓存hash或读取次数削减。

旧9个candidate测试在最终tests文件中逐字作为前缀保留。新用例检查多批路径1/2/4顺序和值一致、empty、mode、nonUTF8、missing和symlink拒绝。现有应跑集合为candidate::tests全组和verify::runner_tests全组（尤其多层真实fd+逐命令checkpoint）；最终strict/fmt仍适用。本审阅者未冒签这些动态结果。

Root外置实测记录：94700文件，串行22.674秒、4worker11.885秒、完整manifest与digest逐项相同，约1.91倍。此为读取Root记录，非本审阅者实跑；不是全阶段1.91倍承诺。现有in-flight runner包含candidate源码身份，不能热改或替换；采用必须形成新候选/重编并遵守原新鲜证据要求。

限制：并行改变跨文件读取时序，不能承诺任意并发改写时取样时刻或首个异常的发生时间和串行一致；原方式本身也不是原子全树snapshot。固定4限制额外fd/CPU/IO压力。新Unix nonUTF8文件创建在部分文件系统可能被拒绝（旧测试原有能力条件），需按实际测试结果区分前置，不能把未运行当成功。

未发现具体实现阻碍。更多逐项静态核对见confirmation.json。

最终后续：Root第二次94700全量串行25.289秒/并行10.507秒，完整manifest+digest仍相同（result-final.json），不推广为全阶段2.4倍。新测试原APFS EILSEQ失败已保留；仅macOS且实际EILSEQ时明确输出文件创建不支持并核path_from_git原始字节，其他异常仍panic；旧9测试逐字不变。此平台能力处理静态通过，r2测试实际结果仍归Root日志。candidate_final源码未变。
