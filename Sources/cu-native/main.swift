import AppKit
import ApplicationServices
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

func axApplication(pid: pid_t) -> AXUIElement {
    let application = AXUIElementCreateApplication(pid)
    // Some hardened or unavailable targets can leave an AX request pending.
    // Keep discovery bounded so a stale app name fails rather than hanging cu.
    _ = AXUIElementSetMessagingTimeout(application, Float(0.20))
    return application
}

func matchingWindow(_ query: String) -> WindowRecord? {
    let candidates = allWindows()
        .enumerated()
        .map { (index: $0.offset, window: $0.element, score: score($0.element, query: query)) }
        .filter { $0.score > 0 }
    guard !candidates.isEmpty else { return nil }

    return candidates.sorted { lhs, rhs in
        if lhs.score != rhs.score { return lhs.score > rhs.score }
        // CGWindowList is front-to-back, so it gives a deterministic active
        // window preference without an additional, potentially blocking AX RPC.
        return lhs.index < rhs.index
    }.first?.window
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

struct AXRecord {
    let path: String
    let role: String
    let name: String
    let frame: CGRect
    let actions: [String]
}

func axValue(_ element: AXUIElement, _ attribute: CFString) -> CFTypeRef? {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, attribute, &value) == .success else { return nil }
    return value
}

func axString(_ element: AXUIElement, _ attribute: CFString) -> String {
    guard let value = axValue(element, attribute) else { return "" }
    if let string = value as? String { return clean(string) }
    return ""
}

func axChildren(_ element: AXUIElement) -> [AXUIElement] {
    guard let value = axValue(element, kAXChildrenAttribute as CFString) else { return [] }
    return value as? [AXUIElement] ?? []
}

func axWindows(pid: pid_t) -> [AXUIElement] {
    let application = axApplication(pid: pid)
    guard let value = axValue(application, kAXWindowsAttribute as CFString) else { return [] }
    return value as? [AXUIElement] ?? []
}

func axPoint(_ element: AXUIElement, _ attribute: CFString) -> CGPoint? {
    guard let value = axValue(element, attribute), CFGetTypeID(value) == AXValueGetTypeID() else { return nil }
    var point = CGPoint.zero
    guard AXValueGetValue(value as! AXValue, .cgPoint, &point) else { return nil }
    return point
}

func axSize(_ element: AXUIElement, _ attribute: CFString) -> CGSize? {
    guard let value = axValue(element, attribute), CFGetTypeID(value) == AXValueGetTypeID() else { return nil }
    var size = CGSize.zero
    guard AXValueGetValue(value as! AXValue, .cgSize, &size) else { return nil }
    return size
}

func axFrame(_ element: AXUIElement) -> CGRect? {
    guard
        let point = axPoint(element, kAXPositionAttribute as CFString),
        let size = axSize(element, kAXSizeAttribute as CFString),
        size.width > 0, size.height > 0
    else { return nil }
    return CGRect(origin: point, size: size).integral
}

func axActions(_ element: AXUIElement) -> [String] {
    var values: CFArray?
    guard AXUIElementCopyActionNames(element, &values) == .success else { return [] }
    return (values as? [String] ?? []).filter { $0.hasPrefix("AX") }
}

func axName(_ element: AXUIElement, role: String) -> String {
    let attributes: [CFString] = [
        kAXTitleAttribute as CFString,
        kAXDescriptionAttribute as CFString,
        kAXHelpAttribute as CFString
    ]
    for attribute in attributes {
        let value = axString(element, attribute)
        if !value.isEmpty { return value }
    }
    // Editable values may contain credentials or drafts. Only use values from
    // non-editable text-like roles when no label/description exists.
    let valueSafeRoles = ["AXStaticText", "AXHeading", "AXLink", "AXButton", "AXMenuItem"]
    if valueSafeRoles.contains(role) {
        let value = axString(element, kAXValueAttribute as CFString)
        if !value.isEmpty && value.count <= 200 { return value }
    }
    return ""
}

func axRecord(_ element: AXUIElement, path: String, knownRole: String? = nil) -> AXRecord? {
    let role = knownRole ?? axString(element, kAXRoleAttribute as CFString)
    guard !role.isEmpty, let frame = axFrame(element) else { return nil }
    return AXRecord(path: path, role: role, name: axName(element, role: role), frame: frame, actions: axActions(element))
}

let interactiveAXRoles: Set<String> = [
    "AXButton", "AXRadioButton", "AXCheckBox", "AXTextField", "AXTextArea",
    "AXMenuButton", "AXMenuItem", "AXPopUpButton", "AXLink", "AXCell", "AXRow",
    "AXComboBox", "AXSlider", "AXTabGroup", "AXDisclosureTriangle"
]

func collectAX(_ element: AXUIElement, path: String, depth: Int, interactiveOnly: Bool, records: inout [AXRecord]) {
    guard depth <= 9, records.count < 1500 else { return }
    let role = axString(element, kAXRoleAttribute as CFString)
    if (!interactiveOnly || interactiveAXRoles.contains(role)),
       let record = axRecord(element, path: path, knownRole: role), !record.name.isEmpty {
        records.append(record)
    }
    for (index, child) in axChildren(element).enumerated() {
        collectAX(child, path: "\(path).\(index)", depth: depth + 1, interactiveOnly: interactiveOnly, records: &records)
        if records.count >= 1500 { return }
    }
}

func axRecords(pid: pid_t, interactiveOnly: Bool = false) -> [AXRecord] {
    var records: [AXRecord] = []
    for (index, window) in axWindows(pid: pid).enumerated() {
        collectAX(window, path: "w\(index)", depth: 0, interactiveOnly: interactiveOnly, records: &records)
    }
    return records
}

func printAXRecord(_ record: AXRecord) {
    let frame = record.frame.integral
    print("\(record.path)\t\(clean(record.role))\t\(clean(record.name))\t\(Int(frame.origin.x))\t\(Int(frame.origin.y))\t\(Int(frame.width))\t\(Int(frame.height))\t\(record.actions.map(clean).joined(separator: ","))")
}

func resolveAX(pid: pid_t, path: String) -> AXUIElement? {
    guard path.first == "w" else { return nil }
    let parts = path.dropFirst().split(separator: ".")
    guard let first = parts.first, let windowIndex = Int(first) else { return nil }
    let windows = axWindows(pid: pid)
    guard windows.indices.contains(windowIndex) else { return nil }
    var element = windows[windowIndex]
    for part in parts.dropFirst() {
        guard let index = Int(part) else { return nil }
        let children = axChildren(element)
        guard children.indices.contains(index) else { return nil }
        element = children[index]
    }
    return element
}

func approximatelyEqual(_ lhs: CGRect, _ rhs: CGRect, tolerance: CGFloat = 4) -> Bool {
    abs(lhs.origin.x - rhs.origin.x) <= tolerance &&
    abs(lhs.origin.y - rhs.origin.y) <= tolerance &&
    abs(lhs.width - rhs.width) <= tolerance &&
    abs(lhs.height - rhs.height) <= tolerance
}

func performAX(element: AXUIElement) -> String? {
    let role = axString(element, kAXRoleAttribute as CFString)
    if ["AXTextField", "AXTextArea", "AXComboBox"].contains(role),
       AXUIElementSetAttributeValue(element, kAXFocusedAttribute as CFString, kCFBooleanTrue) == .success {
        return "AXFocus"
    }
    let actions = axActions(element)
    for action in ["AXPress", "AXOpen", "AXConfirm"] {
        if actions.contains(action), AXUIElementPerformAction(element, action as CFString) == .success {
            return action
        }
    }
    if AXUIElementSetAttributeValue(element, kAXFocusedAttribute as CFString, kCFBooleanTrue) == .success {
        return "AXFocus"
    }
    if actions.contains("AXShowMenu"), AXUIElementPerformAction(element, "AXShowMenu" as CFString) == .success {
        return "AXShowMenu"
    }
    return nil
}

func imageFingerprint(path: String) throws -> String {
    guard
        let image = NSImage(contentsOfFile: path),
        let source = image.cgImage(forProposedRect: nil, context: nil, hints: nil)
    else {
        throw NSError(domain: "cu-native", code: 6, userInfo: [NSLocalizedDescriptionKey: "could not read image"])
    }
    let width = 64, height = 64, bytesPerPixel = 4
    var pixels = [UInt8](repeating: 0, count: width * height * bytesPerPixel)
    guard let context = CGContext(
        data: &pixels,
        width: width,
        height: height,
        bitsPerComponent: 8,
        bytesPerRow: width * bytesPerPixel,
        space: CGColorSpaceCreateDeviceRGB(),
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    ) else {
        throw NSError(domain: "cu-native", code: 7, userInfo: [NSLocalizedDescriptionKey: "could not create image context"])
    }
    context.interpolationQuality = .medium
    context.draw(source, in: CGRect(x: 0, y: 0, width: width, height: height))
    var hash: UInt64 = 1469598103934665603
    for byte in pixels {
        hash ^= UInt64(byte)
        hash &*= 1099511628211
    }
    return String(format: "%016llx", hash)
}

func positiveDelay(_ key: String, default value: TimeInterval) -> TimeInterval {
    guard let raw = ProcessInfo.processInfo.environment[key], let delay = TimeInterval(raw), delay >= 0 else { return value }
    return delay
}

func pasteText(path: String) throws {
    let text = try String(contentsOfFile: path, encoding: .utf8)
    let pasteboard = NSPasteboard.general
    let backup: [[NSPasteboard.PasteboardType: Data]] = (pasteboard.pasteboardItems ?? []).map { item in
        var values: [NSPasteboard.PasteboardType: Data] = [:]
        for type in item.types {
            if let data = item.data(forType: type) { values[type] = data }
        }
        return values
    }
    pasteboard.clearContents()
    guard pasteboard.setString(text, forType: .string) else {
        throw NSError(domain: "cu-native", code: 8, userInfo: [NSLocalizedDescriptionKey: "could not write text to the clipboard"])
    }
    // Give pasteboard consumers a chance to observe the new change count before
    // posting Cmd-V. This is especially important for Chromium/Electron targets.
    Thread.sleep(forTimeInterval: positiveDelay("CU_PASTE_READY_DELAY", default: 0.02))

    guard
        let source = CGEventSource(stateID: .hidSystemState),
        let down = CGEvent(keyboardEventSource: source, virtualKey: 9, keyDown: true),
        let up = CGEvent(keyboardEventSource: source, virtualKey: 9, keyDown: false)
    else {
        throw NSError(domain: "cu-native", code: 9, userInfo: [NSLocalizedDescriptionKey: "could not create paste events"])
    }
    down.flags = .maskCommand; up.flags = .maskCommand
    down.post(tap: .cghidEventTap); up.post(tap: .cghidEventTap)
    // Apps may resolve the pasteboard lazily after receiving Cmd-V. Keep the
    // agent's clipboard payload alive long enough for those consumers, then
    // restore the caller's original clipboard. The delay is configurable for
    // unusually slow remote or web surfaces.
    Thread.sleep(forTimeInterval: positiveDelay("CU_PASTE_RESTORE_DELAY", default: 0.35))

    pasteboard.clearContents()
    if !backup.isEmpty {
        let items = backup.map { values -> NSPasteboardItem in
            let item = NSPasteboardItem()
            for (type, data) in values { item.setData(data, forType: type) }
            return item
        }
        pasteboard.writeObjects(items)
    }
}

func scrollWheel(at point: CGPoint, delta: Int32, count: Int) throws {
    guard AXIsProcessTrusted() else {
        throw NSError(domain: "cu-native", code: 9, userInfo: [NSLocalizedDescriptionKey: "Accessibility permission is unavailable"])
    }
    guard let move = CGEvent(
        mouseEventSource: nil,
        mouseType: .mouseMoved,
        mouseCursorPosition: point,
        mouseButton: .left
    ) else {
        throw NSError(domain: "cu-native", code: 10, userInfo: [NSLocalizedDescriptionKey: "could not create pointer event"])
    }
    move.post(tap: .cghidEventTap)
    Thread.sleep(forTimeInterval: 0.025)

    for index in 0..<count {
        guard let event = CGEvent(
            scrollWheelEvent2Source: nil,
            units: .line,
            wheelCount: 1,
            wheel1: delta,
            wheel2: 0,
            wheel3: 0
        ) else {
            throw NSError(domain: "cu-native", code: 11, userInfo: [NSLocalizedDescriptionKey: "could not create scroll event"])
        }
        event.location = point
        event.post(tap: .cghidEventTap)
        if index + 1 < count { Thread.sleep(forTimeInterval: 0.035) }
    }
}

func usage() -> Never {
    fputs("usage: cu-native windows [app] | bounds APP | display APP | activate APP | app-info APP | context APP [--interactive] | ocr IMAGE X Y | fingerprint IMAGE | paste FILE | scroll X Y DELTA COUNT | ax-status | ax-tree APP | ax-perform PID PATH ROLE NAME X Y W H\n", stderr)
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

case "app-info":
    guard args.count == 2, let window = matchingWindow(args[1]) else { usage() }
    let running = NSRunningApplication(processIdentifier: window.pid)
    print("\(window.pid)\t\(clean(window.app))\t\(clean(running?.bundleIdentifier ?? ""))")

case "context":
    guard (args.count == 2 || args.count == 3), let window = matchingWindow(args[1]) else { usage() }
    let interactiveOnly = args.count == 3 && args[2] == "--interactive"
    guard args.count == 2 || interactiveOnly else { usage() }
    let running = NSRunningApplication(processIdentifier: window.pid)
    let b = window.bounds.integral
    print("META\t\(window.id)\t\(window.pid)\t\(Int(b.origin.x))\t\(Int(b.origin.y))\t\(Int(b.width))\t\(Int(b.height))\t\(clean(window.app))\t\(clean(window.title))\t\(clean(running?.bundleIdentifier ?? ""))")
    guard AXIsProcessTrusted() else { exit(0) }
    for record in axRecords(pid: window.pid, interactiveOnly: interactiveOnly) {
        let f = record.frame.integral
        print("AX\t\(record.path)\t\(record.role)\t\(clean(record.name))\t\(Int(f.origin.x))\t\(Int(f.origin.y))\t\(Int(f.width))\t\(Int(f.height))\t\(record.actions.joined(separator: ","))")
    }

case "ocr":
    guard args.count == 4, let x = Double(args[2]), let y = Double(args[3]) else { usage() }
    do {
        try ocr(path: args[1], originX: x, originY: y)
    } catch {
        fputs("OCR failed: \(error.localizedDescription)\n", stderr)
        exit(1)
    }

case "fingerprint":
    guard args.count == 2 else { usage() }
    do {
        print(try imageFingerprint(path: args[1]))
    } catch {
        fputs("fingerprint failed: \(error.localizedDescription)\n", stderr)
        exit(1)
    }

case "paste":
    guard args.count == 2 else { usage() }
    do {
        try pasteText(path: args[1])
        print("pasted")
    } catch {
        fputs("paste failed: \(error.localizedDescription)\n", stderr)
        exit(1)
    }

case "scroll":
    guard
        args.count == 5,
        let x = Double(args[1]), let y = Double(args[2]),
        let delta = Int32(args[3]), let count = Int(args[4]),
        count > 0
    else { usage() }
    do {
        try scrollWheel(at: CGPoint(x: x, y: y), delta: delta, count: count)
        print("scrolled")
    } catch {
        fputs("scroll failed: \(error.localizedDescription)\n", stderr)
        exit(1)
    }

case "ax-status":
    print(AXIsProcessTrusted() ? "trusted" : "untrusted")
    if !AXIsProcessTrusted() { exit(1) }

case "ax-tree":
    guard (args.count == 2 || args.count == 3), let window = matchingWindow(args[1]) else { usage() }
    guard AXIsProcessTrusted() else {
        fputs("Accessibility permission is unavailable\n", stderr)
        exit(2)
    }
    let records = axRecords(pid: window.pid, interactiveOnly: args.count == 3 && args[2] == "--interactive")
    guard !records.isEmpty else {
        fputs("Accessibility exposed no named elements\n", stderr)
        exit(3)
    }
    for record in records { printAXRecord(record) }

case "ax-perform":
    guard
        args.count == 9,
        let pid = pid_t(args[1]),
        let x = Double(args[5]), let y = Double(args[6]),
        let width = Double(args[7]), let height = Double(args[8])
    else { usage() }
    guard let element = resolveAX(pid: pid, path: args[2]), let record = axRecord(element, path: args[2]) else {
        fputs("stale_element: accessibility path no longer resolves\n", stderr)
        exit(4)
    }
    let expected = CGRect(x: x, y: y, width: width, height: height)
    guard record.role == args[3], record.name == args[4], approximatelyEqual(record.frame, expected) else {
        fputs("stale_element: role, name, or bounds changed\n", stderr)
        exit(4)
    }
    guard let action = performAX(element: element) else {
        fputs("unsupported_action: element exposes no supported action and cannot be focused\n", stderr)
        exit(5)
    }
    print(action)

default:
    usage()
}
