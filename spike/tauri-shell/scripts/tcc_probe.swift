// R01-T06 TCC 只读权限状态探针（macOS）。只查询、不请求授权、不触发弹窗。
// 构建：swiftc -O -o tcc_probe tcc_probe.swift
import Foundation
import AVFoundation
import CoreGraphics
import ApplicationServices
import Speech

func avName(_ s: AVAuthorizationStatus) -> String {
    switch s {
    case .notDetermined: return "not_determined"
    case .restricted: return "restricted"
    case .denied: return "denied"
    case .authorized: return "authorized"
    @unknown default: return "unknown(\(s.rawValue))"
    }
}

var out: [String: String] = [
    "screen_recording_preflight": CGPreflightScreenCaptureAccess() ? "granted" : "denied_or_not_determined",
    "accessibility_trusted": AXIsProcessTrusted() ? "trusted" : "not_trusted",
    "microphone": avName(AVCaptureDevice.authorizationStatus(for: .audio)),
    "camera": avName(AVCaptureDevice.authorizationStatus(for: .video)),
]

// Speech 框架直接链接（只读查询 authorizationStatus，不触发授权请求）。
do {
    let raw = SFSpeechRecognizer.authorizationStatus().rawValue
    let names = [0: "not_determined", 1: "denied", 2: "restricted", 3: "authorized"]
    out["speech_recognition"] = names[raw] ?? "unknown(\(raw))"
}

let data = try! JSONSerialization.data(withJSONObject: out, options: [.sortedKeys])
print(String(data: data, encoding: .utf8)!)
