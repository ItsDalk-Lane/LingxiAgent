// CoordinatorTestRunner.swift — 单次结算协调器的离线测试运行器（F5）
//
// 本机工具链是 Command Line Tools（无 XCTest/swift-testing 模块），因此测试以
// 纯断言可执行文件实现：`swift run lingxi-speech-core-tests`，全部通过退出码 0，
// 任何失败打印明细并退出码 1。
//
// 覆盖：正常终局、单次结算（首个终局获胜）、超时走 cancel 且以 timedOut 结算、
// 取消幂等、终局后晚到事件丢弃、attachTask 与终局的竞态。不触碰真实 Speech
// 框架（mock 任务替身）。

import Foundation
import LingxiSpeechCore

final class MockRecognitionTask: RecognitionTaskCancelling {
    private let lock = NSLock()
    private var cancelCount = 0

    func cancelRecognition() {
        lock.lock()
        cancelCount += 1
        lock.unlock()
    }

    var cancels: Int {
        lock.lock()
        defer { lock.unlock() }
        return cancelCount
    }
}

// 明确等待 continuation 已挂接，不能以调度让步猜测另一个任务已经就绪。
actor ContinuationReady {
    private var ready = false
    private var waiter: CheckedContinuation<Void, Never>?
    func signal() { ready = true; waiter?.resume(); waiter = nil }
    func wait() async {
        if ready { return }
        await withCheckedContinuation { waiter = $0 }
    }
}

@main
struct CoordinatorTestRunner {
    // 串行执行的测试计数器：全部测试在 main 里顺序 await，无并发访问；
    // nonisolated(unsafe) + 锁守护以满足 Swift 6 并发检查。
    static let counterLock = NSLock()
    nonisolated(unsafe) static var total = 0
    nonisolated(unsafe) static var failures = 0

    static func check(_ condition: Bool, _ name: String, _ detail: String = "") {
        counterLock.withLock {
            total += 1
            if !condition { failures += 1 }
        }
        if condition {
            print("PASS \(name)")
        } else {
            print("FAIL \(name)\(detail.isEmpty ? "" : " — \(detail)")")
        }
    }

    static func expectEqual<T: Equatable>(_ actual: T, _ expected: T, _ name: String) {
        check(actual == expected, name, "expected \(expected), got \(actual)")
    }

    static func attachContinuation(
        to coordinator: RecognitionCoordinator, ready: ContinuationReady? = nil
    ) async throws -> String {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<String, Error>) in
            coordinator.attachContinuation(continuation)
            if let ready { Task { await ready.signal() } }
        }
    }

    static func main() async {
        await testFinalResultSettlesWithText()
        await testFirstTerminalWins()
        await testTimeoutCancelsTaskAndSettlesTimedOut()
        await testCancelIsIdempotent()
        await testLateResultAfterTimeoutIsDropped()
        await testTaskAttachedAfterTerminalIsCancelledImmediately()
        await testTerminalBeforeContinuation(cancelled: true)
        await testTerminalBeforeContinuation(cancelled: false)

        print("—")
        let (failedChecks, totalChecks) = counterLock.withLock { (failures, total) }
        if failedChecks > 0 {
            print("FAILED: \(failedChecks)/\(totalChecks) checks failed")
            exit(1)
        }
        print("OK: \(totalChecks)/\(totalChecks) coordinator checks passed")
    }

    static func testFinalResultSettlesWithText() async {
        let coordinator = RecognitionCoordinator()
        let ready = ContinuationReady()
        async let outcome = attachContinuation(to: coordinator, ready: ready)
        await ready.wait()
        coordinator.handleRecognitionEvent(resultText: "你好", isFinal: true, error: nil)
        do {
            let text = try await outcome
            check(true, "final-result: settles")
            expectEqual(text, "你好", "final-result: text")
        } catch {
            check(false, "final-result: settles", "unexpected error \(error)")
        }
        expectEqual(coordinator.currentPhase, .succeeded, "final-result: phase")
    }

    static func testFirstTerminalWins() async {
        let coordinator = RecognitionCoordinator()
        let ready = ContinuationReady()
        async let outcome = attachContinuation(to: coordinator, ready: ready)
        await ready.wait()
        coordinator.handleRecognitionEvent(resultText: "先到结果", isFinal: true, error: nil)
        // 晚到错误/超时/取消：全部必须被丢弃（二次 resume 会触发运行时陷阱）。
        coordinator.handleRecognitionEvent(resultText: nil, isFinal: false,
                                           error: SpeechHelperError.recognitionFailed("late"))
        coordinator.timeout()
        coordinator.cancel()
        do {
            let text = try await outcome
            expectEqual(text, "先到结果", "first-terminal-wins: text")
        } catch {
            check(false, "first-terminal-wins: settles with result", "unexpected error \(error)")
        }
        expectEqual(coordinator.currentPhase, .succeeded, "first-terminal-wins: phase")
    }

    static func testTimeoutCancelsTaskAndSettlesTimedOut() async {
        let coordinator = RecognitionCoordinator()
        let mock = MockRecognitionTask()
        let ready = ContinuationReady()
        async let outcome = attachContinuation(to: coordinator, ready: ready)
        await ready.wait()
        coordinator.attachTask(mock)
        coordinator.timeout()
        do {
            _ = try await outcome
            check(false, "timeout: must not produce success", "unexpectedly succeeded")
        } catch {
            expectEqual(error as? SpeechHelperError, .timedOut, "timeout: error code")
        }
        expectEqual(coordinator.currentPhase, .timedOut, "timeout: phase")
        expectEqual(mock.cancels, 1, "timeout: task.cancel called exactly once (never finish)")
    }

    static func testCancelIsIdempotent() async {
        let coordinator = RecognitionCoordinator()
        let mock = MockRecognitionTask()
        let ready = ContinuationReady()
        async let outcome = attachContinuation(to: coordinator, ready: ready)
        await ready.wait()
        coordinator.attachTask(mock)
        coordinator.cancel()
        coordinator.cancel()
        coordinator.timeout()
        do {
            _ = try await outcome
            check(false, "cancel: must not produce success", "unexpectedly succeeded")
        } catch {
            expectEqual(error as? SpeechHelperError, .cancelled, "cancel: error code")
        }
        expectEqual(coordinator.currentPhase, .cancelled, "cancel: phase")
        expectEqual(mock.cancels, 1, "cancel: idempotent single cancel")
    }

    static func testLateResultAfterTimeoutIsDropped() async {
        let coordinator = RecognitionCoordinator()
        let mock = MockRecognitionTask()
        let ready = ContinuationReady()
        async let outcome = attachContinuation(to: coordinator, ready: ready)
        await ready.wait()
        coordinator.attachTask(mock)
        coordinator.timeout()
        coordinator.handleRecognitionEvent(resultText: "晚到结果", isFinal: true, error: nil)
        do {
            _ = try await outcome
            check(false, "late-result: timeout result must stand", "late result won")
        } catch {
            expectEqual(error as? SpeechHelperError, .timedOut, "late-result: error code")
        }
        expectEqual(coordinator.currentPhase, .timedOut, "late-result: phase")
        expectEqual(mock.cancels, 1, "late-result: cancel count")
    }

    static func testTaskAttachedAfterTerminalIsCancelledImmediately() async {
        let coordinator = RecognitionCoordinator()
        let ready = ContinuationReady()
        async let outcome = attachContinuation(to: coordinator, ready: ready)
        await ready.wait()
        coordinator.timeout()
        let mock = MockRecognitionTask()
        coordinator.attachTask(mock)
        expectEqual(mock.cancels, 1, "race: task attached after terminal cancelled immediately")
        do {
            _ = try await outcome
            check(false, "race: must not produce success", "unexpectedly succeeded")
        } catch {
            expectEqual(error as? SpeechHelperError, .timedOut, "race: error code")
        }
    }
    static func testTerminalBeforeContinuation(cancelled: Bool) async {
        let coordinator = RecognitionCoordinator()
        if cancelled { coordinator.cancel() } else { coordinator.timeout() }
        // 后续终态请求和晚到任务均不能改写最先确定的结果。
        coordinator.cancel()
        coordinator.timeout()
        let mock = MockRecognitionTask()
        coordinator.attachTask(mock)
        do {
            _ = try await attachContinuation(to: coordinator)
            check(false, "pre-attach: must not produce success")
        } catch {
            expectEqual(error as? SpeechHelperError, cancelled ? .cancelled : .timedOut, "pre-attach: exact terminal result")
        }
        expectEqual(mock.cancels, 1, "pre-attach: late task cancelled once")
        expectEqual(coordinator.currentPhase, cancelled ? .cancelled : .timedOut, "pre-attach: terminal phase preserved")
    }

}
