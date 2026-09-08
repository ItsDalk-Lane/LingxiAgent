// SpeechHelperError.swift — helper 的结构化错误（protocol 2 code 字段的来源）
//
// code 与 core/speech-recognition/system-speech-adapter.ts 的
// SYSTEM_SPEECH_ERROR_CODES（去掉 SYSTEM_SPEECH_ 前缀）一一对应；
// 适配器对未认识到的 code 一律降级为 PROCESS_FAILED（不静默）。

public enum SpeechHelperError: Error, Sendable, Equatable {
    case invalidInput(String)
    case permissionRequired
    case permissionDenied
    case restricted
    case recognizerUnavailable
    case timedOut
    case cancelled
    case recognitionFailed(String)
    case outputEncodingFailed

    public var code: String {
        switch self {
        case .invalidInput: return "INVALID_INPUT"
        case .permissionRequired: return "PERMISSION_REQUIRED"
        case .permissionDenied: return "PERMISSION_DENIED"
        case .restricted: return "RESTRICTED"
        case .recognizerUnavailable: return "RECOGNIZER_UNAVAILABLE"
        case .timedOut: return "TIMEOUT"
        case .cancelled: return "CANCELLED"
        case .recognitionFailed: return "RECOGNITION_FAILED"
        case .outputEncodingFailed: return "OUTPUT_ENCODING_FAILED"
        }
    }

    public var message: String {
        switch self {
        case .invalidInput(let detail): return "invalid input: \(detail)"
        case .permissionRequired:
            return "speech recognition permission not yet granted to the host app"
        case .permissionDenied:
            return "speech recognition permission denied (System Settings → Privacy & Security → Speech Recognition)"
        case .restricted:
            return "speech recognition restricted by device policy"
        case .recognizerUnavailable:
            return "no speech recognizer available for the requested locale"
        case .timedOut:
            return "recognition timed out"
        case .cancelled:
            return "recognition cancelled"
        case .recognitionFailed(let detail): return "recognition failed: \(detail)"
        case .outputEncodingFailed: return "failed to encode result"
        }
    }
}
