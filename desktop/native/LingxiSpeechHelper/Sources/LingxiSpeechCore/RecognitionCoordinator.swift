// RecognitionCoordinator.swift — 识别会话的单次结算协调器（F5 核心）
//
// 状态机：created → running → succeeded / failed / timedOut / cancelled。
// 首个终局获胜：到达终局后的一切框架事件（晚到的结果、错误、取消回调）一律丢弃，
// continuation 恰好 resume 一次。取消/超时的实现是 task.cancelRecognition()
// （底层 SFSpeechRecognitionTask.cancel()），绝不调用 finish()——finish 会把
// 未完成识别按「成功收尾」上报，违反「取消/超时不得记为成功」。

import Foundation
import Speech

/// 可取消的识别任务抽象（测试用 mock 替身；生产由 SFSpeechRecognitionTask 适配）。
public protocol RecognitionTaskCancelling: AnyObject {
    func cancelRecognition()
}

extension SFSpeechRecognitionTask: @retroactive @unchecked Sendable {}
extension SFSpeechRecognitionTask: RecognitionTaskCancelling {
    public func cancelRecognition() { cancel() }
}

public enum RecognitionPhase: String, Sendable, Equatable {
    case created
    case running
    case succeeded
    case failed
    case timedOut
    case cancelled
}

public final class RecognitionCoordinator: @unchecked Sendable {
    private let lock = NSLock()
    private var phase: RecognitionPhase = .created
    private var continuation: CheckedContinuation<String, Error>?
    private var task: (any RecognitionTaskCancelling)?
    private var latestText: String = ""
    /// 提前到达的取消/超时：等待方尚未挂接时保存终局，挂接时恰好结算一次。
    private var pendingTerminalError: SpeechHelperError?

    public init() {}

    public var currentPhase: RecognitionPhase {
        lock.lock()
        defer { lock.unlock() }
        return phase
    }

    /// 挂接识别 continuation；进入 running。一个协调器只服务一次识别。
    /// 若取消/超时已先于挂接到达，立即以保存的终局结算本次挂接。
    public func attachContinuation(_ continuation: CheckedContinuation<String, Error>) {
        lock.lock()
        if let pending = pendingTerminalError {
            pendingTerminalError = nil
            lock.unlock()
            continuation.resume(throwing: pending)
            return
        }
        precondition(phase == .created, "continuation attached twice")
        phase = .running
        self.continuation = continuation
        lock.unlock()
    }

    /// 挂接底层识别任务。若终局已先到（超时/取消先于任务创建完成），立即取消之。
    public func attachTask(_ task: any RecognitionTaskCancelling) {
        var cancelNow = false
        lock.lock()
        if phase == .timedOut || phase == .cancelled {
            cancelNow = true
        } else if phase == .created || phase == .running {
            self.task = task
        }
        lock.unlock()
        if cancelNow { task.cancelRecognition() }
    }

    /// 框架事件入口（独立回调队列调用）：error 或 isFinal 构成终局；终局后事件丢弃。
    public func handleRecognitionEvent(resultText: String?, isFinal: Bool, error: Error?) {
        lock.lock()
        if let resultText { latestText = resultText }
        guard phase == .running else {
            lock.unlock()
            return
        }
        if let error {
            phase = .failed
            let pending = continuation
            continuation = nil
            task = nil
            lock.unlock()
            pending?.resume(throwing: SpeechHelperError.recognitionFailed(error.localizedDescription))
            return
        }
        if isFinal {
            phase = .succeeded
            let text = latestText
            let pending = continuation
            continuation = nil
            task = nil
            lock.unlock()
            pending?.resume(returning: text)
            return
        }
        lock.unlock()
    }

    /// 超时：取消底层任务（cancel，非 finish），continuation 以 timedOut 结算。
    public func timeout() {
        terminate(terminalPhase: .timedOut, error: .timedOut)
    }

    /// 外部取消：同上，以 cancelled 结算。幂等。
    public func cancel() {
        terminate(terminalPhase: .cancelled, error: .cancelled)
    }

    private func terminate(terminalPhase: RecognitionPhase, error: SpeechHelperError) {
        lock.lock()
        guard phase == .created || phase == .running else {
            lock.unlock()
            return
        }
        phase = terminalPhase
        let pending = continuation
        continuation = nil
        if pending == nil { pendingTerminalError = error }
        let pendingTask = task
        task = nil
        lock.unlock()
        pendingTask?.cancelRecognition()
        pending?.resume(throwing: error)
    }
}
