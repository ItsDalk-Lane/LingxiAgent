
import AppKit
import Foundation
let args = CommandLine.arguments
guard args.count > 1, let image = NSImage(contentsOfFile: args[1]),
      let tiff = image.tiffRepresentation,
      let rep = NSBitmapImageRep(data: tiff) else { print("{\"ok\":false}"); exit(0) }
let w = rep.pixelsWide, h = rep.pixelsHigh
var r = 0.0, g = 0.0, b = 0.0, n = 0.0
let stepX = max(1, w / 64), stepY = max(1, h / 64)
var y = 0
while y < h { var x = 0; while x < w {
  if let c = rep.colorAt(x: x, y: y) { r += c.redComponent; g += c.greenComponent; b += c.blueComponent; n += 1 }
  x += stepX }; y += stepY }
if n <= 0 { print("{\"ok\":false}"); exit(0) }
print("{\"ok\":true,\"width\":\(w),\"height\":\(h),\"r\":\(r/n),\"g\":\(g/n),\"b\":\(b/n)}")
