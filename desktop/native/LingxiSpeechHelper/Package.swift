// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "LingxiSpeechHelper",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "lingxi-speech-helper", targets: ["LingxiSpeechHelper"]),
        // 离线断言式测试运行器（CLT 无 XCTest/swift-testing 模块）：
        // swift run lingxi-speech-core-tests
        .executable(name: "lingxi-speech-core-tests", targets: ["LingxiSpeechCoreRunner"]),
    ],
    targets: [
        // 识别核心（协调器/转写/输出协议）：独立库 target，供测试覆盖。
        .target(
            name: "LingxiSpeechCore",
            path: "Sources/LingxiSpeechCore"
        ),
        // 命令行薄壳：参数解析 + 终局落盘。
        .executableTarget(
            name: "LingxiSpeechHelper",
            dependencies: ["LingxiSpeechCore"],
            path: "Sources/LingxiSpeechHelper"
        ),
        .executableTarget(
            name: "LingxiSpeechCoreRunner",
            dependencies: ["LingxiSpeechCore"],
            path: "Tests/LingxiSpeechCoreRunner"
        ),
    ]
)
