// R01-T06 spike 二进制入口：全部逻辑在 lib（便于集成测试复用握手校验等纯逻辑）。
fn main() {
    spike_lib::run();
}
