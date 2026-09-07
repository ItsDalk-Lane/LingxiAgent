// LingxiSpeechHelper — macOS 系统语音识别（SFSpeechRecognizer）命令行封装
//
// 用法：
//   lingxi-speech-helper transcribe --input <wav-path> [--locale zh-CN] [--timeout <seconds>]
//   lingxi-speech-helper status            → {"available":true,"locale":"zh-CN"}
//   lingxi-speech-helper version
//
// 成功时向 stdout 打印单行 JSON：{"ok":true,"text":"...","durationMs":123}
// 失败时退出码非 0，stderr 带原因；stdout 无输出（fail-closed，无静默降级）。

import Foundation
import Speech
import AVFoundation

struct TranscribeResult: Codable {
    let ok: Bool
    let text: String
    let durationMs: Int
}

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(("lingxi-speech-helper error: " + message + "\n").data(using: .utf8)!)
    exit(2)
}

func parseArgs() -> (command: String, input: String?, locale: String?, timeout: TimeInterval) {
    var args = Array(CommandLine.arguments.dropFirst())
    guard let command = args.first else {
        fail("missing command (transcribe|status|version)")
    }
    args.removeFirst()
    var input: String? = nil
    var locale: String? = nil
    var timeout: TimeInterval = 120

    var index = 0
    while index < args.count {
        switch args[index] {
        case "--input":
            index += 1
            guard index < args.count else { fail("--input requires a value") }
            input = args[index]
        case "--locale":
            index += 1
            guard index < args.count else { fail("--locale requires a value") }
            locale = args[index]
        case "--timeout":
            index += 1
            guard index < args.count else { fail("--timeout requires a value") }
            if let value = TimeInterval(args[index]), value > 0 { timeout = value }
        default:
            fail("unknown argument: \(args[index])")
        }
        index += 1
    }
    return (command, input, locale, timeout)
}

/// SFSpeechRecognizer 文件转写；信号量等待回调，保持 main 同步语义。
func runTranscribe(inputPath: String, localeIdentifier: String?, timeout: TimeInterval) {
    let fileURL = URL(fileURLWithPath: inputPath)
    guard FileManager.default.fileExists(atPath: inputPath) else {
        fail("input file does not exist: \(inputPath)")
    }

    // 关键约束：绝不在本进程里调用 requestAuthorization —— 裸 CLI 无应用上下文时
    // Speech 框架会直接 SIGABRT（实测）。打包形态下 helper 由 Lingxi.app spawn，
    // TCC 把授权归属到宿主 App（App 首次使用语音功能时引导用户授权）。
    let effectiveStatus = SFSpeechRecognizer.authorizationStatus()
    guard effectiveStatus == .authorized else {
        let reason: String
        switch effectiveStatus {
        case .denied: reason = "speech recognition permission denied (System Settings → Privacy & Security → Speech Recognition)"
        case .restricted: reason = "speech recognition restricted by device policy"
        case .notDetermined: reason = "speech recognition permission not yet granted to the host app; trigger any speech feature once to see the grant prompt"
        default: reason = "speech recognition unauthorized"
        }
        fail(reason)
    }

    let locale = localeIdentifier.flatMap { Locale(identifier: $0) } ?? Locale.current
    guard let recognizer = SFSpeechRecognizer(locale: locale), recognizer.isAvailable else {
        fail("no speech recognizer available for locale \(locale.identifier)")
    }
    let request = SFSpeechURLRecognitionRequest(url: fileURL)
    request.shouldReportPartialResults = false
    if #available(macOS 13, *), recognizer.supportsOnDeviceRecognition {
        request.requiresOnDeviceRecognition = true
        request.addsPunctuation = true
    }

    // 时长非必需字段；Swift 6 严格并发下异步取 duration 的桥接不值得引入，
    // 恒为 0（消费方按缺失处理）。
    let durationMs = 0

    let semaphore = DispatchSemaphore(value: 0)
    var finalText: String? = nil
    var recognitionError: Error? = nil
    var finished = false
    let lock = NSLock()

    let task = recognizer.recognitionTask(with: request) { result, error in
        lock.lock()
        defer { lock.unlock() }
        if let result = result {
            finalText = result.bestTranscription.formattedString
        }
        if let error = error {
            recognitionError = error
        }
        // 文件转写的终止信号：error 或 result.isFinal（SFSpeechRecognitionResult
        // 没有 isFinished，终止判定用 isFinal）。
        let done = error != nil || (result?.isFinal ?? false)
        if done && !finished {
            finished = true
            semaphore.signal()
        }
    }
    _ = task

    let waitResult = semaphore.wait(timeout: .now() + timeout)
    if waitResult == .timedOut {
        task.finish()
        fail("recognition timed out after \(Int(timeout))s")
    }
    if let recognitionError = recognitionError {
        fail("recognition failed: \(recognitionError.localizedDescription)")
    }
    let text = finalText ?? ""
    let payload = TranscribeResult(ok: true, text: text, durationMs: durationMs)
    guard let data = try? JSONEncoder().encode(payload) else {
        fail("failed to encode result")
    }
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write("\n".data(using: .utf8)!)
}

let parsed = parseArgs()
switch parsed.command {
case "version":
    print("lingxi-speech-helper 1.0.0")
case "status":
    let status = SFSpeechRecognizer.authorizationStatus()
    let authorized = status == .authorized
    let sayAvailable = FileManager.default.fileExists(atPath: "/usr/bin/say")
    print("{\"available\":\(authorized),\"authStatus\":\(status.rawValue),\"sayAvailable\":\(sayAvailable),\"locale\":\"\(Locale.current.identifier)\"}")
case "transcribe":
    guard let input = parsed.input else { fail("transcribe requires --input <file>") }
    runTranscribe(inputPath: input, localeIdentifier: parsed.locale, timeout: parsed.timeout)
default:
    fail("unknown command: \(parsed.command)")
}

// App Sandbox / hardened runtime 下 AVAudioApplication 麦克风键不影响文件转写；
// 此处只做文件转写，不采集麦克风。
exit(0)
