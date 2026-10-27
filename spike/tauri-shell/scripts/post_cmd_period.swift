import CoreGraphics
import Foundation
let pid = pid_t(CommandLine.arguments[1])!
let src = CGEventSource(stateID: .combinedSessionState)
let down = CGEvent(keyboardEventSource: src, virtualKey: 47, keyDown: true)  // period
let up = CGEvent(keyboardEventSource: src, virtualKey: 47, keyDown: false)
down?.flags = .maskCommand
up?.flags = .maskCommand
down?.postToPid(pid)
usleep(80_000)
up?.postToPid(pid)
print("posted Cmd+. to pid \(pid) at \(Date())")
