// R01-T06 全局快捷键触发探针：合成 CGEvent（Cmd+Alt+Shift+9，ANSI keycode 25）。
// 合成键盘事件需要注入进程（本探针）拥有辅助功能权限；若无权限事件会被系统丢弃——
// 是否触发以宿主报告中 shortcut.fired 事件为准，本脚本只负责注入并报告本地观察。
import CoreGraphics
import Foundation

let keyCode: CGKeyCode = 25 // ANSI 9
let flags: CGEventFlags = [.maskCommand, .maskAlternate, .maskShift]

let src = CGEventSource(stateID: .combinedSessionState)
let down = CGEvent(keyboardEventSource: src, virtualKey: keyCode, keyDown: true)
let up = CGEvent(keyboardEventSource: src, virtualKey: keyCode, keyDown: false)
down?.flags = flags
up?.flags = flags
down?.post(tap: .cghidEventTap)
usleep(80_000)
up?.post(tap: .cghidEventTap)
print("posted synthetic Cmd+Alt+Shift+9 keyDown/keyUp at \(Date())")
