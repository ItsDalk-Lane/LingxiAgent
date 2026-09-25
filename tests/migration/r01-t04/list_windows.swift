// list_windows.swift — R01-T04 可见性证据辅助：枚举指定 pid 的在屏窗口（CGWindowList）。
// 编译：swiftc -O -o list_windows list_windows.swift
// 用法：list_windows <pid>
import CoreGraphics
import Foundation

guard CommandLine.arguments.count > 1, let pid = Int32(CommandLine.arguments[1]) else {
    FileHandle.standardError.write("usage: list_windows <pid>\n".data(using: .utf8)!)
    exit(1)
}
let opts: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
guard let list = CGWindowListCopyWindowInfo(opts, kCGNullWindowID) as? [[String: Any]] else {
    print("window-list-unavailable")
    exit(2)
}
var found = 0
for w in list {
    guard let owner = w[kCGWindowOwnerPID as String] as? Int32, owner == pid else { continue }
    let layer = w[kCGWindowLayer as String] as? Int ?? -1
    let bounds = w[kCGWindowBounds as String] as? [String: Any] ?? [:]
    let width = bounds["Width"] as? Int ?? 0
    let height = bounds["Height"] as? Int ?? 0
    let name = w[kCGWindowName as String] as? String ?? ""
    if layer == 0 && width > 100 && height > 100 {
        found += 1
        print("onscreen=1 layer=0 w=\(width) h=\(height) name=\(name)")
    }
}
if found == 0 {
    print("onscreen=0 (no layer-0 window for pid \(pid))")
    exit(3)
}
