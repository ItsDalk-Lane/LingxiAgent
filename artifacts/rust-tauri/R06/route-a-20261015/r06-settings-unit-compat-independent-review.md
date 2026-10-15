# 设置旧测试夹具接口适配只读复核

PASS_STATIC_FIXTURE_API_ONLY。净patch仅四个测试文件增加缺失的store.subscribe/getter连接键接口与一处测试类型，删除0行，原assertion字节不改，production/guard/门禁无变化。

subscribe是空兼容mock，不证明真实订阅回调；连接键mock只反映本旧夹具的activeServerConnectionId，不能替代生产registry/token隔离检查。本轮未运行95文件，Root随后全量运行结果需单独记录，不能把静态结论当UI全绿。

机器结论 `/tmp/r06-settings-unit-compat-independent-confirmation.json`；没有应用Main。
