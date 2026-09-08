// LingxiSpeechHelperMain.swift — lingxi-speech-helper 命令行入口（薄壳）
//
// 用法：
//   lingxi-speech-helper transcribe --input <wav-path> [--locale zh-CN] [--timeout <seconds>]
//   lingxi-speech-helper status
//   lingxi-speech-helper version
//
// 全部输出走 stdout 单行 JSON（protocol 2，见 HelperOutput.swift）；失败退出码 2，
// stderr 只写人类可读诊断。识别逻辑在 LingxiSpeechCore（可测）；这里只做参数解析
// 与终局落盘。

import Foundation
import Speech
import LingxiSpeechCore

@main
struct LingxiSpeechHelperMain {
    static let helperVersion = "2.0.0"

    struct Arguments {
        var command: String
        var input: String?
        var locale: String?
        var timeout: TimeInterval = 120
    }

    static func parseArguments() throws -> Arguments {
        var args = Array(CommandLine.arguments.dropFirst())
        guard let command = args.first else {
            throw SpeechHelperError.invalidInput("missing command (transcribe|status|version)")
        }
        args.removeFirst()
        var parsed = Arguments(command: command)
        var index = 0
        while index < args.count {
            switch args[index] {
            case "--input":
                index += 1
                guard index < args.count else { throw SpeechHelperError.invalidInput("--input requires a value") }
                parsed.input = args[index]
            case "--locale":
                index += 1
                guard index < args.count else { throw SpeechHelperError.invalidInput("--locale requires a value") }
                parsed.locale = args[index]
            case "--timeout":
                index += 1
                guard index < args.count else { throw SpeechHelperError.invalidInput("--timeout requires a value") }
                if let value = TimeInterval(args[index]), value > 0 { parsed.timeout = value }
            case let unknown:
                throw SpeechHelperError.invalidInput("unknown argument: \(unknown)")
            }
            index += 1
        }
        return parsed
    }

    static func main() async {
        do {
            let arguments = try parseArguments()
            switch arguments.command {
            case "version":
                try writeStdoutLine(HelperVersionOutput(version: helperVersion))
            case "status":
                let status = SFSpeechRecognizer.authorizationStatus()
                try writeStdoutLine(HelperStatusOutput(
                    available: status == .authorized,
                    authStatus: Int(status.rawValue),
                    locale: Locale.current.identifier
                ))
            case "transcribe":
                guard let input = arguments.input else {
                    throw SpeechHelperError.invalidInput("transcribe requires --input <file>")
                }
                let output = try await SpeechTranscriber().transcribe(
                    inputPath: input,
                    localeIdentifier: arguments.locale,
                    timeout: arguments.timeout
                )
                try writeStdoutLine(HelperSuccessOutput(
                    text: output.text,
                    resultCode: output.resultCode,
                    durationMs: 0
                ))
            default:
                throw SpeechHelperError.invalidInput("unknown command: \(arguments.command)")
            }
        } catch let error as SpeechHelperError {
            exitWithFailure(error)
        } catch {
            exitWithFailure(.recognitionFailed(error.localizedDescription))
        }
    }

    static func exitWithFailure(_ error: SpeechHelperError) -> Never {
        if let encoded = try? writeStdoutLine(HelperFailureOutput(code: error.code, message: error.message)) {
            _ = encoded
        } else {
            writeStderrLine("failed to encode failure output for \(error.code)")
        }
        writeStderrLine(error.message)
        exit(2)
    }
}
