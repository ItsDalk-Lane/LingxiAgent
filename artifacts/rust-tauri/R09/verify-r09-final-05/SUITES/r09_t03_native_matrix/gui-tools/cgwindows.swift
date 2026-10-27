
import CoreGraphics
import Foundation
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
var out: [[String: Any]] = []
for w in list {
  let owner = w[kCGWindowOwnerName as String] as? String ?? ""
  let name = w[kCGWindowName as String] as? String ?? ""
  let layer = w[kCGWindowLayer as String] as? Int ?? -1
  let onscreen = w[kCGWindowIsOnscreen as String] as? Bool ?? false
  let pid = w[kCGWindowOwnerPID as String] as? Int ?? -1
  let number = w[kCGWindowNumber as String] as? Int ?? -1
  var bounds: [String: Int] = [:]
  if let b = w[kCGWindowBounds as String] as? [String: Any] {
    bounds = ["x": Int((b["X"] as? Double) ?? 0), "y": Int((b["Y"] as? Double) ?? 0),
              "width": Int((b["Width"] as? Double) ?? 0), "height": Int((b["Height"] as? Double) ?? 0)]
  }
  out.append(["owner": owner, "name": name, "layer": layer, "onscreen": onscreen,
              "pid": pid, "number": number, "bounds": bounds])
}
let data = try! JSONSerialization.data(withJSONObject: out, options: [])
print(String(data: data, encoding: .utf8)!)
