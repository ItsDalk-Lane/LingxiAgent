import CoreGraphics
import Foundation
let src = CGEventSource(stateID: .combinedSessionState)
let down = CGEvent(keyboardEventSource: src, virtualKey: 53, keyDown: true)
let up = CGEvent(keyboardEventSource: src, virtualKey: 53, keyDown: false)
down?.post(tap: .cghidEventTap)
usleep(80_000)
up?.post(tap: .cghidEventTap)
print("posted Escape at \(Date())")
