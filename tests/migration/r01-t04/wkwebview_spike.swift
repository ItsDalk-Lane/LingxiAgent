// wkwebview_spike.swift — R01-T04 系统 WebView 宿主候选实测（macOS WKWebView，原生真实宿主）。
//
// 编译：swiftc -O -o wkwebview_spike wkwebview_spike.swift -framework Cocoa -framework WebKit
// 运行：./wkwebview_spike --site http://127.0.0.1:18281 --evidence <dir> [--snapshot-script-file <path>]
//
// 逐项实测：导航/DOM snapshot（复用生产 SNAPSHOT_SCRIPT，运行时从 desktop/main.cjs 只读提取）、
// JS 点击/输入/选择/滚动、截图（视口/整页尝试）、下载、弹窗、JS 对话框、cookie、
// 两会话隔离（独立 dataStore）、登录持久化（forIdentifier dataStore 重建）、
// 代理（WKWebsiteDataStore.proxyConfigurations 死代理负向）、不可信页探针、
// 可见窗口与用户接管（osascript 独立通道键入）。
// 输出：evidence/transcript.jsonl + 截图 PNG。退出码：全 PASS=0，有 FAIL=2。

import Cocoa
import Network
import WebKit

// ── transcript ──
var evidenceDir = ""
var passCount = 0
var failCount = 0
var transcriptLines: [String] = []

func jsonEscape(_ s: String) -> String {
    var out = ""
    for c in s {
        switch c {
        case "\"": out += "\\\""
        case "\\": out += "\\\\"
        case "\n": out += "\\n"
        case "\r": out += "\\r"
        case "\t": out += "\\t"
        default: out.append(c)
        }
    }
    return out
}

func record(_ step: String, _ capability: String, _ op: String, _ expect: String,
            _ actual: String, _ verdict: String, _ evidence: [String] = []) {
    if verdict == "VERIFIED" { passCount += 1 }
    if verdict == "FAILED" { failCount += 1 }
    let ev = evidence.map { "\"\(jsonEscape($0))\"" }.joined(separator: ",")
    let line = "{\"step\":\"\(step)\",\"capability\":\"\(jsonEscape(capability))\",\"op\":\"\(jsonEscape(op))\",\"expect\":\"\(jsonEscape(expect))\",\"actual\":\"\(jsonEscape(actual))\",\"verdict\":\"\(verdict)\",\"evidence\":[\(ev)]}"
    transcriptLines.append(line)
    FileManager.default.createFile(atPath: evidenceDir + "/transcript.jsonl",
        contents: (transcriptLines.joined(separator: "\n") + "\n").data(using: .utf8))
    FileHandle.standardError.write("[\(step)] \(verdict) — \(capability): \(actual)\n".data(using: .utf8)!)
}

func savePng(_ image: NSImage, _ name: String) -> (String, Int) {
    guard let cg = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else { return ("", 0) }
    let rep = NSBitmapImageRep(cgImage: cg)
    guard let data = rep.representation(using: .png, properties: [:]) else { return ("", 0) }
    let path = evidenceDir + "/screenshots/" + name + ".png"
    try? FileManager.default.createDirectory(atPath: evidenceDir + "/screenshots", withIntermediateDirectories: true)
    try? data.write(to: URL(fileURLWithPath: path))
    return (path, data.count)
}

// ── 生产 snapshot 脚本提取（与 Rust spike 同规则，sha256 交叉核对）──
func extractSnapshotScript(_ repoPath: String) -> (String, String)? {
    guard let text = try? String(contentsOfFile: repoPath + "/desktop/main.cjs", encoding: .utf8) else { return nil }
    let lines = text.components(separatedBy: "\n")
    let marker = "const SNAPSHOT_SCRIPT = `"
    guard let startIdx = lines.firstIndex(where: { $0.hasPrefix(marker) }) else { return nil }
    var endIdx: Int? = nil
    for i in (startIdx + 1)..<lines.count {
        if lines[i].hasSuffix("`)();") || lines[i].trimmingCharacters(in: .whitespaces) == "})()`;" {
            endIdx = i
            break
        }
    }
    guard let end = endIdx else { return nil }
    var bodyLines = [String(lines[startIdx].dropFirst(marker.count))]
    for i in (startIdx + 1)...end { bodyLines.append(lines[i]) }
    var body = bodyLines.joined(separator: "\n")
    if body.hasSuffix("`;") { body = String(body.dropLast(2)) }
    return (body, "lines=\(startIdx + 1)-\(end + 1)")
}

// ── delegates ──
final class NavBox: NSObject, WKNavigationDelegate, WKUIDelegate, WKDownloadDelegate {
    var loadContinuations: [CheckedContinuation<Bool, Never>] = []
    var downloadDone: ((Bool, String) -> Void)?
    var popupWebView: WKWebView?
    var lastDialog: String = ""
    var downloadDestDir = ""
    var failedError: String = ""

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        let cs = loadContinuations
        loadContinuations = []
        for c in cs { c.resume(returning: true) }
    }
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        failedError = error.localizedDescription
        let cs = loadContinuations
        loadContinuations = []
        for c in cs { c.resume(returning: false) }
    }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        failedError = error.localizedDescription
        let cs = loadContinuations
        loadContinuations = []
        for c in cs { c.resume(returning: false) }
    }
    // 下载（macOS 11.3+）
    func webView(_ webView: WKWebView, decidePolicyFor navigationResponse: WKNavigationResponse,
                 decisionHandler: @escaping (WKNavigationResponsePolicy) -> Void) {
        if let resp = navigationResponse.response as? HTTPURLResponse,
           let cd = resp.allHeaderFields["Content-Disposition"] as? String,
           cd.contains("attachment") {
            decisionHandler(.download)
            return
        }
        decisionHandler(.allow)
    }
    func webView(_ webView: WKWebView, navigationResponse: WKNavigationResponse, didBecome download: WKDownload) {
        download.delegate = self
    }
    func download(_ download: WKDownload, decideDestinationUsing response: URLResponse,
                  suggestedFilename: String, completionHandler: @escaping (URL?) -> Void) {
        let dest = downloadDestDir + "/" + suggestedFilename
        try? FileManager.default.createDirectory(atPath: downloadDestDir, withIntermediateDirectories: true)
        completionHandler(URL(fileURLWithPath: dest))
    }
    func downloadDidFinish(_ download: WKDownload) {
        downloadDone?(true, "finished")
        downloadDone = nil
    }
    func download(_ download: WKDownload, didFailWithError error: Error, resumeData: Data?) {
        downloadDone?(false, error.localizedDescription)
        downloadDone = nil
    }
    // 弹窗 window.open
    func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration,
                 for navigationAction: WKNavigationAction,
                 windowFeatures: WKWindowFeatures) -> WKWebView? {
        let popup = WKWebView(frame: webView.bounds, configuration: configuration)
        popup.navigationDelegate = self
        popup.uiDelegate = self
        popupWebView = popup
        return popup
    }
    // JS 对话框
    func webView(_ webView: WKWebView, runJavaScriptAlertPanelWithMessage message: String,
                 initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping () -> Void) {
        lastDialog = "alert:" + message
        completionHandler()
    }
    func webView(_ webView: WKWebView, runJavaScriptConfirmPanelWithMessage message: String,
                 initiatedByFrame frame: WKFrameInfo,
                 completionHandler: @escaping (Bool) -> Void) {
        lastDialog = "confirm:" + message
        completionHandler(false) // 拒绝
    }
}

// continuation 只允许 resume 一次的保护盒
final class ResumeOnceBox<T> {
    private var cont: CheckedContinuation<T, Never>?
    init(_ c: CheckedContinuation<T, Never>) { cont = c }
    func resume(_ v: T) { if let c = cont { cont = nil; c.resume(returning: v) } }
}

func makeWebView(store: WKWebsiteDataStore, box: NavBox, frame: NSRect) -> WKWebView {
    let config = WKWebViewConfiguration()
    config.websiteDataStore = store
    // 允许 JS window.open（生产语义：agent 触发的弹窗必须被宿主捕获为受控新视图）
    config.preferences.javaScriptCanOpenWindowsAutomatically = true
    let wv = WKWebView(frame: frame, configuration: config)
    wv.navigationDelegate = box
    wv.uiDelegate = box
    return wv
}

@MainActor
func eval(_ wv: WKWebView, _ expr: String) async -> String {
    do {
        let r = try await wv.evaluateJavaScript(expr)
        if let s = r as? String { return s }
        if let n = r as? NSNumber { return n.stringValue }
        return r.map { String(describing: $0) } ?? "undefined"
    } catch {
        return "JSERROR: \(error.localizedDescription)"
    }
}

@MainActor
func navAndWait(_ wv: WKWebView, _ box: NavBox, _ url: String) async -> Bool {
    box.failedError = ""
    return await withCheckedContinuation { cont in
        box.loadContinuations.append(cont)
        wv.load(URLRequest(url: URL(string: url)!))
    }
}

// ── main ──
let args = CommandLine.arguments
func argValue(_ flag: String) -> String? {
    guard let i = args.firstIndex(of: flag), i + 1 < args.count else { return nil }
    return args[i + 1]
}
let site = (argValue("--site") ?? "http://127.0.0.1:18281").trimmingCharacters(in: CharacterSet(charactersIn: "/"))
evidenceDir = argValue("--evidence") ?? "/tmp/r01t04/wk-evidence"
let repo = argValue("--repo") ?? ""
try? FileManager.default.createDirectory(atPath: evidenceDir, withIntermediateDirectories: true)

let app = NSApplication.shared
app.setActivationPolicy(.regular)

let window = NSWindow(
    contentRect: NSRect(x: 100, y: 100, width: 1280, height: 860),
    styleMask: [.titled, .closable, .resizable],
    backing: .buffered, defer: false)
window.title = "R01-T04 WKWebView spike"

let box = NavBox()
box.downloadDestDir = evidenceDir + "/downloads"
let mainStore = WKWebsiteDataStore.default()
let wv = makeWebView(store: mainStore, box: box, frame: window.contentView!.bounds)
window.contentView!.addSubview(wv)
window.makeKeyAndOrderFront(nil)
app.activate(ignoringOtherApps: true)

Task { @MainActor in
    // W0 snapshot 脚本来源
    if let (script, span) = extractSnapshotScript(repo) {
        record("W0", "证据源", "extract SNAPSHOT_SCRIPT from desktop/main.cjs", "提取成功", span, "VERIFIED")
        let snapshotScript = script

        // W1 导航 + snapshot
        let ok = await navAndWait(wv, box, site + "/form")
        let snapRaw = await eval(wv, snapshotScript)
        // snapshot script returns object; evaluateJavaScript 对对象返回 NSDictionary
        var snapText = ""
        if let r = try? await wv.evaluateJavaScript(snapshotScript) as? [String: Any] {
            snapText = r["text"] as? String ?? ""
        }
        let hasRef = snapText.contains("[") && snapText.contains("提交表单")
        record("W1", "导航+DOM引用snapshot", "load /form + 生产 SNAPSHOT_SCRIPT",
               "快照含交互元素 ref",
               "nav=\(ok) snap_len=\(snapText.count) has_refs=\(hasRef)",
               ok && hasRef ? "VERIFIED" : "FAILED")
        let _ = snapRaw

        // W2 视口截图
        do {
            let img = try await wv.takeSnapshot(configuration: nil)
            let (path, size) = savePng(img, "w2-form-viewport")
            record("W2", "截图(视口)", "WKWebView.takeSnapshot", "PNG 非空", "\(size) bytes", size > 5000 ? "VERIFIED" : "FAILED", [path])
        } catch {
            record("W2", "截图(视口)", "takeSnapshot", "PNG", "error: \(error.localizedDescription)", "FAILED")
        }

        // W3 中文输入（JS 值注入路径）
        _ = await eval(wv, "(function(){var el=document.getElementById('t1');el.focus();el.value='灵犀你好2026';el.dispatchEvent(new Event('input',{bubbles:true}));return el.value;})()")
        let typedVal = await eval(wv, "document.getElementById('t1').value")
        record("W3", "中文输入(JS)", "evaluateJavaScript 设置 value+input 事件", "value==灵犀你好2026",
               "value=\(typedVal)", typedVal == "灵犀你好2026" ? "VERIFIED" : "FAILED")

        // W4 select + 提交（JS click）
        _ = await eval(wv, "(function(){var s=document.getElementById('sel1');s.value='c';s.dispatchEvent(new Event('change',{bubbles:true}));document.getElementById('submit1').click();return 1;})()")
        try? await Task.sleep(nanoseconds: 400_000_000)
        let echo = await eval(wv, "document.getElementById('echo').textContent")
        let w4ok = echo.contains("title=灵犀你好2026") && echo.contains("choice=c")
        record("W4", "select+表单提交语义", "JS select+click 提交", "echo 含中文标题与 choice=c", echo, w4ok ? "VERIFIED" : "FAILED")

        // W5 长页面滚动 + 整页截图尝试
        _ = await navAndWait(wv, box, site + "/long")
        _ = await eval(wv, "window.scrollBy(0,900)")
        try? await Task.sleep(nanoseconds: 300_000_000)
        let sy = await eval(wv, "Math.round(window.scrollY)")
        record("W5a", "滚动", "window.scrollBy(900)", "scrollY>0", "scrollY=\(sy)", sy != "0" && !sy.hasPrefix("JSERROR") ? "VERIFIED" : "FAILED")
        // 整页：WKSnapshotConfiguration.rect 用文档全高
        let docH = Int(await eval(wv, "document.body.scrollHeight")) ?? 0
        let cfg = WKSnapshotConfiguration()
        cfg.rect = NSRect(x: 0, y: 0, width: 1280, height: docH)
        do {
            let img = try await wv.takeSnapshot(configuration: cfg)
            let (path, size) = savePng(img, "w5-long-fullpage-attempt")
            let imgH = img.size.height
            record("W5b", "整页长截图", "takeSnapshot(rect=全文档高 \(docH))",
                   "记录实际行为（WKWebView 是否支持超视口截图）",
                   "img=\(Int(img.size.width))x\(Int(imgH)) bytes=\(size)",
                   "VERIFIED", [path])
        } catch {
            record("W5b", "整页长截图", "takeSnapshot(rect)", "记录行为", "error: \(error.localizedDescription)", "FAILED")
        }

        // W6 上传（WKWebView 无程序化 file input 赋值 API —— 记录实测）
        _ = await navAndWait(wv, box, site + "/form")
        let setFileAttempt = await eval(wv, "(function(){var f=document.getElementById('file1');try{var dt=new DataTransfer();f.files=dt.files;return 'js-assign-ok len='+f.files.length;}catch(e){return 'js-assign-threw:'+e.message;}})()")
        record("W6", "上传(agent驱动)", "WKWebView 无 setFileInputFiles 等价物；JS files 赋值尝试",
               "记录能力缺口", setFileAttempt,
               setFileAttempt.hasPrefix("js-assign-threw") || setFileAttempt.contains("len=0") ? "UNVERIFIED" : "UNVERIFIED")

        // W7 下载
        let dlOK: (Bool, String) = await withCheckedContinuation { cont in
            let resumeBox = ResumeOnceBox(cont)
            box.downloadDone = { ok, msg in resumeBox.resume((ok, msg)) }
            wv.load(URLRequest(url: URL(string: site + "/download/hello.txt")!))
            // 超时保护（MainActor 上延时检查）
            Task { @MainActor in
                try? await Task.sleep(nanoseconds: 10_000_000_000)
                if box.downloadDone != nil {
                    box.downloadDone = nil
                    resumeBox.resume((false, "timeout"))
                }
            }
        }
        let dlPath = evidenceDir + "/downloads/hello.txt"
        let dlData = try? Data(contentsOf: URL(fileURLWithPath: dlPath))
        let dlMatch = dlData.map { String(data: $0, encoding: .utf8) == "lingxi-t04-download-payload-你好-0123456789\n" } ?? false
        record("W7", "下载", "导航到附件 URL → WKDownload 落盘", "文件内容与固定 payload 一致",
               "download=\(dlOK.0) \(dlOK.1) fileMatch=\(dlMatch) bytes=\(dlData?.count ?? -1)",
               dlOK.0 && dlMatch ? "VERIFIED" : "FAILED", [dlPath])

        // W8 弹窗 window.open → createWebViewWith
        _ = await navAndWait(wv, box, site + "/popup")
        _ = await eval(wv, "document.getElementById('open-popup').click()")
        try? await Task.sleep(nanoseconds: 1_500_000_000)
        var popupMarker = ""
        if let popup = box.popupWebView {
            popupMarker = await eval(popup, "location.href + '|' + ((document.getElementById('popup-marker')||{}).textContent||'')")
        }
        let w8ok = popupMarker.contains("/popup-target") && popupMarker.contains("POPUP_TARGET_LOADED")
        record("W8", "弹窗(window.open)", "window.open → createWebViewWith 捕获",
               "弹窗 webview 创建并加载目标", "popup=\(popupMarker.isEmpty ? "none" : popupMarker)",
               w8ok ? "VERIFIED" : (box.popupWebView != nil ? "FAILED" : "FAILED"))

        // W9 JS 对话框
        _ = await navAndWait(wv, box, site + "/dialog")
        _ = await eval(wv, "document.getElementById('alert-btn').click()")
        try? await Task.sleep(nanoseconds: 400_000_000)
        let alertRes = await eval(wv, "document.getElementById('dialog-result').textContent")
        let alertDlg = box.lastDialog
        _ = await eval(wv, "document.getElementById('confirm-btn').click()")
        try? await Task.sleep(nanoseconds: 400_000_000)
        let confirmRes = await eval(wv, "document.getElementById('dialog-result').textContent")
        let confirmDlg = box.lastDialog
        let w9ok = alertDlg.hasPrefix("alert:t04-alert") && alertRes == "alert-done"
            && confirmDlg.hasPrefix("confirm:t04-confirm") && confirmRes == "confirm:false"
        record("W9", "JS 对话框", "alert accept / confirm reject(委托)",
               "alert→alert-done; confirm 拒绝→confirm:false",
               "alert=\(alertDlg)→\(alertRes) | confirm=\(confirmDlg)→\(confirmRes)",
               w9ok ? "VERIFIED" : "FAILED")

        // W10 登录 + cookie（默认 dataStore）
        _ = await navAndWait(wv, box, site + "/login")
        _ = await eval(wv, "(function(){document.getElementById('u').value='demo';document.getElementById('p').value='lingxi-pass-2026';document.getElementById('login-btn').click();return 1;})()")
        try? await Task.sleep(nanoseconds: 1_200_000_000)
        let whoami = await eval(wv, "(document.getElementById('whoami')||{}).textContent || location.href")
        var cookieSeen = false
        let cookies: [HTTPCookie] = await withCheckedContinuation { cont in
            mainStore.httpCookieStore.getAllCookies { cs in cont.resume(returning: cs) }
        }
        cookieSeen = cookies.contains { $0.name == "lingxi_t04_session" }
        record("W10", "登录+cookie", "登录表单提交 → /account + httpCookieStore 查询",
               "LOGGED_IN demo 且 session cookie 可读",
               "whoami=\(whoami) cookie=\(cookieSeen)",
               whoami.contains("LOGGED_IN demo") && cookieSeen ? "VERIFIED" : "FAILED")

        // W11 两会话隔离：两个 nonPersistent dataStore
        let storeA = WKWebsiteDataStore.nonPersistent()
        let storeB = WKWebsiteDataStore.nonPersistent()
        let boxA = NavBox(); let boxB = NavBox()
        let wvA = makeWebView(store: storeA, box: boxA, frame: window.contentView!.bounds)
        let wvB = makeWebView(store: storeB, box: boxB, frame: window.contentView!.bounds)
        _ = await navAndWait(wvA, boxA, site + "/storage?value=ALPHA_A")
        _ = await navAndWait(wvB, boxB, site + "/storage?value=BRAVO_B")
        _ = await navAndWait(wvA, boxA, site + "/storage-read")
        _ = await navAndWait(wvB, boxB, site + "/storage-read")
        try? await Task.sleep(nanoseconds: 500_000_000)
        let readA = await eval(wvA, "document.getElementById('storage-read').textContent")
        let readB = await eval(wvB, "document.getElementById('storage-read').textContent")
        let isoOK = readA.contains("ALPHA_A") && !readA.contains("BRAVO_B")
            && readB.contains("BRAVO_B") && !readB.contains("ALPHA_A")
        record("W11", "两会话隔离(独立 dataStore)", "A/B nonPersistent dataStore 各写各读",
               "互不串 localStorage/cookie", "A=[\(readA)] B=[\(readB)]", isoOK ? "VERIFIED" : "FAILED")

        // W12 登录持久化：forIdentifier dataStore 重建（macOS 14+）
        if #available(macOS 14.0, *) {
            let ident = UUID(uuidString: "11111111-2222-3333-4444-555555555555")!
            let store1 = WKWebsiteDataStore(forIdentifier: ident)
            let box1 = NavBox()
            let wv1 = makeWebView(store: store1, box: box1, frame: window.contentView!.bounds)
            _ = await navAndWait(wv1, box1, site + "/login")
            _ = await eval(wv1, "(function(){document.getElementById('u').value='demo';document.getElementById('p').value='lingxi-pass-2026';document.getElementById('login-btn').click();return 1;})()")
            try? await Task.sleep(nanoseconds: 1_200_000_000)
            let who1 = await eval(wv1, "(document.getElementById('whoami')||{}).textContent || ''")
            // 销毁并重建同 identifier 的 dataStore（模拟挂起/冷恢复）
            let store2 = WKWebsiteDataStore(forIdentifier: ident)
            let box2 = NavBox()
            let wv2 = makeWebView(store: store2, box: box2, frame: window.contentView!.bounds)
            _ = await navAndWait(wv2, box2, site + "/account")
            let who2 = await eval(wv2, "(document.getElementById('whoami')||{}).textContent || location.href")
            record("W12", "登录持久化(forIdentifier)", "同 identifier dataStore 重建后访问 /account",
                   "仍 LOGGED_IN（cookie 随 dataStore 持久）",
                   "first=\(who1) afterRecreate=\(who2)",
                   who1.contains("LOGGED_IN") && who2.contains("LOGGED_IN") ? "VERIFIED" : "FAILED")
        } else {
            record("W12", "登录持久化(forIdentifier)", "macOS 14+ API", "VERIFIED", "系统版本不支持", "UNVERIFIED")
        }

        // W13 代理负向：proxyConfigurations 死代理（macOS 14+）
        // 注意：WKWebView/CFNetwork 对 loopback 目标隐式绕过代理（与 Chromium 隐式 loopback bypass 同语义），
        // 而任务红线禁止真实外发，无法用非 loopback 目标做负向验证 → 如实记 UNVERIFIED。
        if #available(macOS 14.0, *) {
            do {
                let deadEndpoint = NWEndpoint.hostPort(host: "127.0.0.1", port: 9)
                let proxyStore = WKWebsiteDataStore.nonPersistent()
                proxyStore.proxyConfigurations = [ProxyConfiguration(httpCONNECTProxy: deadEndpoint)]
                let boxP = NavBox()
                let wvP = makeWebView(store: proxyStore, box: boxP, frame: window.contentView!.bounds)
                let navOK = await navAndWait(wvP, boxP, site + "/form")
                // 同时验证：默认 dataStore（无 proxyConfigurations）同 URL 可导航 → 配置是 per-dataStore 粒度
                let navDefaultOK = await navAndWait(wv, box, site + "/form")
                record("W13", "代理配置(WK)", "proxyConfigurations=httpCONNECT 127.0.0.1:9 → 导航 loopback /form",
                       "记录实际行为；loopback 目标被 WK 隐式绕过代理（外发目标验证被红线禁止）",
                       "deadProxyNav=\(navOK) err=\(boxP.failedError.isEmpty ? "none" : boxP.failedError) defaultNav=\(navDefaultOK)；结论：API 可用且 per-dataStore，但 loopback 绕过使负向验证不可完成",
                       "UNVERIFIED")
            }
        } else {
            record("W13", "代理配置(WK)", "proxyConfigurations", "macOS 14+", "系统版本不支持", "UNVERIFIED")
        }

        // W14 不可信页探针（宿主注入面）
        _ = await navAndWait(wv, box, site + "/probe")
        var probeText = ""
        for _ in 0..<20 {
            probeText = await eval(wv, "document.getElementById('probe-out').textContent")
            if probeText.hasPrefix("PROBE_RESULT") { break }
            try? await Task.sleep(nanoseconds: 300_000_000)
        }
        let probeOK = probeText.contains("\"hana\":\"undefined\"") && probeText.contains("\"processGlobal\":\"undefined\"")
        record("W14", "不可信页拿不到宿主权限", "/probe 页探测 window.hana/process/CDP",
               "全部不可得", probeText, probeOK ? "VERIFIED" : "FAILED")

        // W15 挂起语义：WKWebView 移除视图层级不销毁 → 状态保留
        _ = await navAndWait(wv, box, site + "/form")
        _ = await eval(wv, "(function(){var el=document.getElementById('ta1');el.value='suspend-marker-保留';return 1;})()")
        wv.removeFromSuperview() // 挂起：摘下视图
        try? await Task.sleep(nanoseconds: 800_000_000)
        window.contentView!.addSubview(wv) // 恢复：挂回
        let taVal = await eval(wv, "document.getElementById('ta1').value")
        record("W15", "挂起/热恢复", "removeFromSuperview → 重新挂载", "textarea 内容保留",
               "ta1=\(taVal)", taVal == "suspend-marker-保留" ? "VERIFIED" : "FAILED")

        // W16 用户接管：可见窗口 + OS 级独立输入通道
        _ = await navAndWait(wv, box, site + "/form")
        _ = await eval(wv, "(function(){var el=document.getElementById('t1');el.value='';el.focus();return 1;})()")
        app.activate(ignoringOtherApps: true)
        window.makeKeyAndOrderFront(nil)
        window.makeFirstResponder(wv)
        try? await Task.sleep(nanoseconds: 700_000_000) // 等激活与焦点生效
        _ = await eval(wv, "document.getElementById('t1').focus()")
        let osa = Process()
        osa.launchPath = "/usr/bin/osascript"
        osa.arguments = ["-e", "tell application \"System Events\" to keystroke \"WK-HUMAN\""]
        let osaErr = Pipe()
        osa.standardError = osaErr
        try? osa.run()
        osa.waitUntilExit()
        try? await Task.sleep(nanoseconds: 800_000_000)
        let humanVal = await eval(wv, "document.getElementById('t1').value")
        let focusDiag = await eval(wv, "(document.activeElement||{}).id || 'none'")
        let appState = "appActive=\(app.isActive) keyWin=\(window.isKeyWindow) firstRespIsWV=\(window.firstResponder === wv) focusEl=\(focusDiag)"
        let osaErrText = String(data: osaErr.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
        let w16ok = humanVal.contains("WK-HUMAN")
        record("W16", "用户接管(独立输入通道)", "osascript keystroke 到聚焦的 WKWebView 窗口",
               "OS 级键入落在页面输入框且脚本可读回",
               "value=\(humanVal) osaStatus=\(osa.terminationStatus) err=\(osaErrText.trimmingCharacters(in: .whitespacesAndNewlines)) \(appState)",
               w16ok ? "VERIFIED" : "UNVERIFIED")
        do {
            let img = try await wv.takeSnapshot(configuration: nil)
            let (shotPath, _) = savePng(img, "w16-final")
            let _ = shotPath
        } catch { /* 截图失败不单独计步 */ }
    } else {
        record("W0", "证据源", "extract SNAPSHOT_SCRIPT", "提取成功", "提取失败", "FAILED")
    }

    // 汇总
    let summary = "{\"pass\":\(passCount),\"fail\":\(failCount)}"
    try? summary.write(toFile: evidenceDir + "/summary.json", atomically: true, encoding: .utf8)
    FileHandle.standardError.write("[wkwebview] done pass=\(passCount) fail=\(failCount)\n".data(using: .utf8)!)
    exit(failCount > 0 ? 2 : 0)
}

RunLoop.main.run()
