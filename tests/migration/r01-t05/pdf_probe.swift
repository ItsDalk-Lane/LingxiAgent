// pdf_probe.swift — R01-T05 PDF 探针（PDFKit/Quartz，仅系统框架，离线）。
// 用法：pdf_probe <pdf> <outdir> [scale]
// 产出：
//   <outdir>/probe.json  页数、逐页 MediaBox、文本长度/sha256、墨迹覆盖率、墨迹包围盒、16x16 亮度网格
//   <outdir>/text.txt    全文提取（PDFKit string）
//   <outdir>/page-NN.png 逐页位图（scale 倍，默认 2 ≈ 144dpi）
// 退出码：0 正常；2 PDF 打不开/无页（结构失败）。
import Foundation
import PDFKit
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers
import CryptoKit

func sha256hex(_ data: Data) -> String {
    SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
}

let args = CommandLine.arguments
guard args.count >= 3 else {
    FileHandle.standardError.write("usage: pdf_probe <pdf> <outdir> [scale]\n".data(using: .utf8)!)
    exit(64)
}
let pdfPath = args[1]
let outDir = URL(fileURLWithPath: args[2])
let scale = args.count > 3 ? Double(args[3]) ?? 2.0 : 2.0

try? FileManager.default.createDirectory(at: outDir, withIntermediateDirectories: true)

let url = URL(fileURLWithPath: pdfPath)
guard let doc = PDFDocument(url: url), doc.pageCount > 0 else {
    FileHandle.standardError.write("pdf_probe: cannot open or empty: \(pdfPath)\n".data(using: .utf8)!)
    exit(2)
}

let fullText = doc.string ?? ""
try? fullText.write(to: outDir.appendingPathComponent("text.txt"), atomically: true, encoding: .utf8)

var pages: [[String: Any]] = []
for i in 0..<doc.pageCount {
    guard let page = doc.page(at: i) else { continue }
    let mb = page.bounds(for: .mediaBox)
    let text = page.string ?? ""
    try? text.write(to: outDir.appendingPathComponent(String(format: "page-%02d.txt", i + 1)),
                    atomically: true, encoding: .utf8)

    // 渲染到位图
    let w = Int((mb.width * CGFloat(scale)).rounded())
    let h = Int((mb.height * CGFloat(scale)).rounded())
    let bytesPerRow = w * 4
    var data = [UInt8](repeating: 255, count: w * h * 4)
    var inkCount = 0
    var minX = w, minY = h, maxX = -1, maxY = -1
    let gridN = 16
    var grid = [Double](repeating: 0, count: gridN * gridN)
    var gridCnt = [Double](repeating: 0, count: gridN * gridN)

    if let ctx = CGContext(data: &data, width: w, height: h, bitsPerComponent: 8,
                           bytesPerRow: bytesPerRow, space: CGColorSpaceCreateDeviceRGB(),
                           bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) {
        ctx.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1))
        ctx.fill(CGRect(x: 0, y: 0, width: w, height: h))
        ctx.saveGState()
        ctx.scaleBy(x: CGFloat(scale), y: CGFloat(scale))
        page.draw(with: .mediaBox, to: ctx)
        ctx.restoreGState()

        for y in 0..<h {
            for x in 0..<w {
                let o = y * bytesPerRow + x * 4
                let r = data[o], g = data[o + 1], b = data[o + 2]
                let luma = (Double(r) * 0.299 + Double(g) * 0.587 + Double(b) * 0.114)
                let gy = min(gridN - 1, y * gridN / max(h, 1))
                let gx = min(gridN - 1, x * gridN / max(w, 1))
                grid[gy * gridN + gx] += luma
                gridCnt[gy * gridN + gx] += 1
                if luma < 245 {
                    inkCount += 1
                    if x < minX { minX = x }
                    if y < minY { minY = y }
                    if x > maxX { maxX = x }
                    if y > maxY { maxY = y }
                }
            }
        }

        // 写 PNG
        if let cgimg = ctx.makeImage() {
            let pngURL = outDir.appendingPathComponent(String(format: "page-%02d.png", i + 1))
            if let dest = CGImageDestinationCreateWithURL(pngURL as CFURL, UTType.png.identifier as CFString, 1, nil) {
                CGImageDestinationAddImage(dest, cgimg, nil)
                CGImageDestinationFinalize(dest)
            }
        }
    }
    let totalPx = max(w * h, 1)
    let gridMean = zip(grid, gridCnt).map { $1 > 0 ? $0 / $1 : 255 }.map { ($0 * 10).rounded() / 10 }
    pages.append([
        "index": i + 1,
        "mediaBox": [mb.origin.x, mb.origin.y, mb.width, mb.height].map { ($0 * 100).rounded() / 100 },
        "bitmapSize": [w, h],
        "textLen": text.count,
        "textSha256": sha256hex(Data(text.utf8)),
        "inkCoverage": Double(inkCount) / Double(totalPx),
        "inkBBox": maxX >= 0 ? [minX, minY, maxX, maxY] : [],
        "lumaGrid16": gridMean,
    ])
}

let probe: [String: Any] = [
    "pdf": pdfPath,
    "pageCount": doc.pageCount,
    "allTextLen": fullText.count,
    "allTextSha256": sha256hex(Data(fullText.utf8)),
    "scale": scale,
    "pages": pages,
]
let jsonData = try JSONSerialization.data(withJSONObject: probe, options: [.prettyPrinted])
try jsonData.write(to: outDir.appendingPathComponent("probe.json"))
print("{\"pageCount\":\(doc.pageCount),\"allTextSha256\":\"\(sha256hex(Data(fullText.utf8)))\"}")
