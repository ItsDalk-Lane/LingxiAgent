
import AppKit
import Foundation
let app = NSWorkspace.shared.frontmostApplication
print(app?.localizedName ?? "")
