// SpeechTranscriber.swift — 异步文件转写（F5：无信号量、无轮询、无 finish()）
//
// 结构：SFSpeechRecognitionTask 的回调落在独立 OperationQueue；识别结果经
// CheckedContinuation 进入 async 世界；超时用 TaskGroup 竞速实现，超时分支
// 触发 coordinator.timeout()（task.cancel + 单次结算）。识别上限由调用方
// （Node 适配器 --timeout 参数）给出，进程级上限在 Node 侧另有 150s 兜底。

import Foundation
import Speech

public struct TranscriptionOutput: Sendable, Equatable {
    public var text: String
    public var resultCode: String?

    public init(text: String, resultCode: String?) {
        self.text = text
        self.resultCode = resultCode
    }
}

public struct SpeechTranscriber: Sendable {
    public init() {}

    public func transcribe(inputPath: String, localeIdentifier: String?, timeout: TimeInterval) async throws -> TranscriptionOutput {
        guard FileManager.default.fileExists(atPath: inputPath) else {
            throw SpeechHelperError.invalidInput("input file does not exist: \(inputPath)")
        }

        // 关键约束：绝不在本进程里调用 requestAuthorization——裸 CLI 无应用上下文时
        // Speech 框架会直接 SIGABRT（实测）。授权由宿主 App（Lingxi.app 主进程，
        // desktop/speech-permissions.cjs）在录音前完成；这里只读状态并显式失败。
        switch SFSpeechRecognizer.authorizationStatus() {
        case .authorized:
            break
        case .denied:
            throw SpeechHelperError.permissionDenied
        case .restricted:
            throw SpeechHelperError.restricted
        case .notDetermined:
            throw SpeechHelperError.permissionRequired
        @unknown default:
            throw SpeechHelperError.permissionRequired
        }

        let locale = localeIdentifier.map { Locale(identifier: $0) } ?? .current
        guard SFSpeechRecognizer(locale: locale) != nil else {
            throw SpeechHelperError.recognizerUnavailable
        }

        let coordinator = RecognitionCoordinator()
        do {
            let text = try await withThrowingTaskGroup(of: String.self, returning: String.self) { group in
                group.addTask {
                    // recognizer/request 非 Sendable：在本任务内部创建并持有，
                    // 不跨任务捕获（Swift 6 sending 闭包约束）。
                    guard let recognizer = SFSpeechRecognizer(locale: locale), recognizer.isAvailable else {
                        throw SpeechHelperError.recognizerUnavailable
                    }
                    // 识别回调落独立队列：不占主线程、不依赖 RunLoop。
                    let callbackQueue = OperationQueue()
                    callbackQueue.name = "com.lingxi.speech-helper.recognition"
                    callbackQueue.maxConcurrentOperationCount = 1
                    recognizer.queue = callbackQueue

                    let request = SFSpeechURLRecognitionRequest(url: URL(fileURLWithPath: inputPath))
                    request.shouldReportPartialResults = false
                    // F7/P5.4：听写保留本地处理目标——始终要求设备内识别；
                    // 语言/机型不支持时不静默改用云端，显式报「识别器不可用」。
                    guard recognizer.supportsOnDeviceRecognition else {
                        throw SpeechHelperError.recognizerUnavailable
                    }
                    request.requiresOnDeviceRecognition = true
                    request.addsPunctuation = true

                    return try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<String, Error>) in
                        coordinator.attachContinuation(continuation)
                        let task = recognizer.recognitionTask(with: request) { result, error in
                            coordinator.handleRecognitionEvent(
                                resultText: result?.bestTranscription.formattedString,
                                isFinal: result?.isFinal ?? false,
                                error: error
                            )
                        }
                        coordinator.attachTask(task)
                    }
                }
                group.addTask {
                    try await Task.sleep(nanoseconds: UInt64(max(timeout, 0.001) * 1_000_000_000))
                    try Task.checkCancellation()
                    coordinator.timeout()
                    throw SpeechHelperError.timedOut
                }
                let first = try await group.next()!
                group.cancelAll()
                return first
            }
            // 空文本是明确结果（EMPTY_RESULT），不伪造文本。
            return TranscriptionOutput(text: text, resultCode: text.isEmpty ? "EMPTY_RESULT" : nil)
        } catch let error as SpeechHelperError {
            // 保险：任何失败路径都确保识别任务被取消（协调器终局幂等）。
            coordinator.cancel()
            throw error
        } catch {
            coordinator.cancel()
            throw SpeechHelperError.recognitionFailed(error.localizedDescription)
        }
    }
}
