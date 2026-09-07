// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "LingxiSpeechHelper",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "lingxi-speech-helper", targets: ["LingxiSpeechHelper"]),
    ],
    targets: [
        .executableTarget(
            name: "LingxiSpeechHelper",
            path: "Sources/LingxiSpeechHelper"
        ),
    ]
)
