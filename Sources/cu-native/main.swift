import AppKit
import CoreGraphics
import Foundation
import Vision

struct WindowRecord {
    let id: CGWindowID
    let pid: pid_t
    let app: String
    let title: String
    let bounds: CGRect
    let layer: Int
}

func clean(_ value: String) -> String {
    value.replacingOccurrences(of: "\t", with: " ")
        .replacingOccurrences(of: "\n", with: " ")
        .replacingOccurrences(of: "\r", with: " ")
}

func canonical(_ value: String) -> String {
    value.lowercased().unicodeScalars
        .filter { CharacterSet.alphanumerics.contains($0) }
        .map(String.init)
        .joined()
}

func allWindows() -> [WindowRecord] {
    let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
    guard let rows = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] else {
        return []
    }

    return rows.compactMap { row in
        guard
            let number = row[kCGWindowNumber as String] as? NSNumber,
            let ownerPID = row[kCGWindowOwnerPID as String] as? NSNumber,
            let owner = row[kCGWindowOwnerName as String] as? String,
            let boundsValue = row[kCGWindowBounds as String],
            let bounds = CGRect(dictionaryRepresentation: boundsValue as! CFDictionary)
        else { return nil }

        let layer = (row[kCGWindowLayer as String] as? NSNumber)?.intValue ?? 0
        let alpha = (row[kCGWindowAlpha as String] as? NSNumber)?.doubleValue ?? 1
        guard layer == 0, alpha > 0, bounds.width >= 40, bounds.height >= 40 else { return nil }

        let pid = pid_t(ownerPID.int32Value)
        let running = NSRunningApplication(processIdentifier: pid)
        let app = running?.localizedName ?? owner
        let title = (row[kCGWindowName as String] as? String) ?? ""
        return WindowRecord(
            id: CGWindowID(number.uint32Value),
            pid: pid,
            app: app,
            title: title,
            bounds: bounds,
            layer: layer
        )
    }
}

func score(_ window: WindowRecord, query: String) -> Int {
    let q = query.lowercased()
    let cq = canonical(query)
    let app = window.app.lowercased()
    let title = window.title.lowercased()
    let bundle = NSRunningApplication(processIdentifier: window.pid)?.bundleIdentifier?.lowercased() ?? ""
    if app == q { return 100 }
    if canonical(app) == cq { return 98 }
    if bundle == q { return 95 }
    if canonical(bundle).hasSuffix(cq) { return 92 }
    if app.hasPrefix(q) { return 85 }
    if canonical(app).hasPrefix(cq) { return 82 }
    if app.contains(q) { return 75 }
    if bundle.contains(q) { return 70 }
    if title == q { return 65 }
    if title.contains(q) { return 55 }
    return 0
}

func matchingWindow(_ query: String) -> WindowRecord? {
    allWindows()
        .enumerated()
        .map { (index: $0.offset, window: $0.element, score: score($0.element, query: query)) }
        .filter { $0.score > 0 }
        .sorted {
            if $0.score != $1.score { return $0.score > $1.score }
            // CGWindowList is front-to-back, so preserve its order for equal matches.
            return $0.index < $1.index
        }
        .first?.window
}

func printWindow(_ window: WindowRecord) {
    let b = window.bounds.integral
    print("\(window.id)\t\(window.pid)\t\(Int(b.origin.x))\t\(Int(b.origin.y))\t\(Int(b.width))\t\(Int(b.height))\t\(clean(window.app))\t\(clean(window.title))")
}

func activate(_ window: WindowRecord) -> Bool {
    guard let app = NSRunningApplication(processIdentifier: window.pid) else { return false }
    return app.activate(options: [.activateAllWindows])
}

func displayContaining(_ window: WindowRecord) -> (CGDirectDisplayID, CGRect)? {
    var count: UInt32 = 0
    guard CGGetActiveDisplayList(0, nil, &count) == .success else { return nil }
    var displays = [CGDirectDisplayID](repeating: 0, count: Int(count))
    guard CGGetActiveDisplayList(count, &displays, &count) == .success else { return nil }
    let point = CGPoint(x: window.bounds.midX, y: window.bounds.midY)
    if let display = displays.first(where: { CGDisplayBounds($0).contains(point) }) {
        return (display, CGDisplayBounds(display).integral)
    }
    guard let main = displays.first else { return nil }
    return (main, CGDisplayBounds(main).integral)
}

func ocr(path: String, originX: Double, originY: Double) throws {
    guard
        let image = NSImage(contentsOfFile: path),
        let cgImage = image.cgImage(forProposedRect: nil, context: nil, hints: nil)
    else {
        throw NSError(domain: "cu-native", code: 2, userInfo: [NSLocalizedDescriptionKey: "could not read image"])
    }

    let request = VNRecognizeTextRequest()
    request.recognitionLevel = .accurate
    request.usesLanguageCorrection = false
    request.recognitionLanguages = ["en-US"]
    try VNImageRequestHandler(cgImage: cgImage, options: [:]).perform([request])

    let width = Double(cgImage.width)
    let height = Double(cgImage.height)
    let observations = (request.results ?? []).sorted {
        let ay = 1 - $0.boundingBox.maxY
        let by = 1 - $1.boundingBox.maxY
        if abs(ay - by) > 0.01 { return ay < by }
        return $0.boundingBox.minX < $1.boundingBox.minX
    }

    for observation in observations {
        guard let candidate = observation.topCandidates(1).first else { continue }
        let box = observation.boundingBox
        let x = originX + (box.midX * width)
        let y = originY + ((1 - box.midY) * height)
        let w = box.width * width
        let h = box.height * height
        print("\(clean(candidate.string))\t\(Int(x.rounded()))\t\(Int(y.rounded()))\t\(Int(w.rounded()))\t\(Int(h.rounded()))\t\(String(format: "%.3f", candidate.confidence))")
    }
}

func usage() -> Never {
    fputs("usage: cu-native windows [app] | bounds APP | display APP | activate APP | ocr IMAGE ORIGIN_X ORIGIN_Y\n", stderr)
    exit(64)
}

let args = Array(CommandLine.arguments.dropFirst())
guard let command = args.first else { usage() }

switch command {
case "windows":
    let query = args.count > 1 ? args[1] : ""
    let rows = query.isEmpty ? allWindows() : allWindows().filter { score($0, query: query) > 0 }
    for window in rows { printWindow(window) }

case "bounds":
    guard args.count == 2 else { usage() }
    guard let window = matchingWindow(args[1]) else {
        fputs("no CoreGraphics window matched '\(args[1])'\n", stderr)
        exit(1)
    }
    printWindow(window)

case "activate":
    guard args.count == 2 else { usage() }
    guard let window = matchingWindow(args[1]) else {
        fputs("no CoreGraphics window matched '\(args[1])'\n", stderr)
        exit(1)
    }
    guard activate(window) else {
        fputs("could not activate '\(args[1])'\n", stderr)
        exit(1)
    }
    printWindow(window)

case "display":
    guard args.count == 2 else { usage() }
    guard let window = matchingWindow(args[1]), let (display, bounds) = displayContaining(window) else {
        fputs("no display matched '\(args[1])'\n", stderr)
        exit(1)
    }
    print("\(display)\t\(Int(bounds.origin.x))\t\(Int(bounds.origin.y))\t\(Int(bounds.width))\t\(Int(bounds.height))")

case "ocr":
    guard args.count == 4, let x = Double(args[2]), let y = Double(args[3]) else { usage() }
    do {
        try ocr(path: args[1], originX: x, originY: y)
    } catch {
        fputs("OCR failed: \(error.localizedDescription)\n", stderr)
        exit(1)
    }

default:
    usage()
}
