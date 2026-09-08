// HelperOutput.swift — helper 的 stdout 线协议（protocol 2，单行 JSON）
//
// 成功：{"ok":true,"protocol":2,"text":"...","durationMs":0[,"resultCode":"EMPTY_RESULT"]}
// 失败：{"ok":false,"protocol":2,"code":"...","message":"..."}（exit 2）
// Node 适配器（system-speech-adapter.ts）以 stdout 结构化 JSON 为主协议；
// stderr 仅人类可读诊断，不再承载机器语义。

import Foundation

public struct HelperSuccessOutput: Encodable {
    public let ok = true
    public let protocolVersion = 2
    public let text: String
    public let resultCode: String?
    public let durationMs: Int

    public init(text: String, resultCode: String?, durationMs: Int) {
        self.text = text
        self.resultCode = resultCode
        self.durationMs = durationMs
    }

    enum CodingKeys: String, CodingKey {
        case ok
        case protocolVersion = "protocol"
        case text
        case resultCode
        case durationMs
    }
}

public struct HelperFailureOutput: Encodable {
    public let ok = false
    public let protocolVersion = 2
    public let code: String
    public let message: String

    public init(code: String, message: String) {
        self.code = code
        self.message = message
    }

    enum CodingKeys: String, CodingKey {
        case ok
        case protocolVersion = "protocol"
        case code
        case message
    }
}

public struct HelperStatusOutput: Encodable {
    public let ok = true
    public let protocolVersion = 2
    public let available: Bool
    public let authStatus: Int
    public let locale: String

    public init(available: Bool, authStatus: Int, locale: String) {
        self.available = available
        self.authStatus = authStatus
        self.locale = locale
    }

    enum CodingKeys: String, CodingKey {
        case ok
        case protocolVersion = "protocol"
        case available
        case authStatus
        case locale
    }
}

public struct HelperVersionOutput: Encodable {
    public let ok = true
    public let protocolVersion = 2
    public let version: String

    public init(version: String) {
        self.version = version
    }

    enum CodingKeys: String, CodingKey {
        case ok
        case protocolVersion = "protocol"
        case version
    }
}

public func writeStdoutLine<T: Encodable>(_ payload: T) throws {
    let data = try JSONEncoder().encode(payload)
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data("\n".utf8))
}

public func writeStderrLine(_ message: String) {
    FileHandle.standardError.write(Data(("lingxi-speech-helper: \(message)\n").utf8))
}
