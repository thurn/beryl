// Native actions requested by the the client library.

import UIKit

extension TableController {
    /// The request's string is borrowed from Rust, so copy it before another FFI call.
    func perform(_ request: BerylRequest) {
        switch Int(request.kind) {
        case BERYL_REQUEST_SHARE:
            guard let text = request.text else { return }
            share(URL(fileURLWithPath: String(cString: text)))
        case BERYL_REQUEST_HAPTIC:
            let style: UIImpactFeedbackGenerator.FeedbackStyle =
                request.haptic == UInt32(BERYL_HAPTIC_MEDIUM) ? .medium : .light
            UIImpactFeedbackGenerator(style: style).impactOccurred()
            #if DEBUG
                logHostEvent("haptic")
            #endif
        case BERYL_REQUEST_OPEN_URL:
            guard let text = request.text, let url = URL(string: String(cString: text)) else {
                return
            }
            UIApplication.shared.open(url) { opened in
                #if DEBUG
                    logHostEvent(opened ? "open-url" : "open-url failed: \(url)")
                #endif
            }
        case BERYL_REQUEST_BEGIN_TEXT_ENTRY, BERYL_REQUEST_END_TEXT_ENTRY:
            break
        default:
            break
        }
    }

    private func share(_ file: URL) {
        guard FileManager.default.fileExists(atPath: file.path) else {
            print("beryl: cannot share missing file \(file.path)")
            return
        }
        guard let root = view.window?.rootViewController, root.presentedViewController == nil else {
            print("beryl: cannot present the share sheet")
            return
        }
        let sheet = UIActivityViewController(activityItems: [file], applicationActivities: nil)
        sheet.popoverPresentationController?.sourceView = root.view
        root.present(sheet, animated: true) {
            #if DEBUG
                logHostEvent("share")
            #endif
        }
    }

    #if DEBUG
        func simulateRequest(_ event: String) {
            var request = BerylRequest()
            switch event {
            case "haptic":
                request.kind = UInt32(BERYL_REQUEST_HAPTIC)
                request.haptic = UInt32(BERYL_HAPTIC_MEDIUM)
                perform(request)
            case "share":
                let logs = try? FileManager.default.contentsOfDirectory(
                    at: documentsDirectory, includingPropertiesForKeys: nil)
                guard let file = logs?.filter({ $0.pathExtension == "jsonl" })
                    .max(by: { $0.lastPathComponent < $1.lastPathComponent })
                else {
                    logHostEvent("share failed: no frame log")
                    return
                }
                request.kind = UInt32(BERYL_REQUEST_SHARE)
                file.path.withCString {
                    request.text = $0
                    perform(request)
                }
            case "open-url":
                request.kind = UInt32(BERYL_REQUEST_OPEN_URL)
                "https://example.com".withCString {
                    request.text = $0
                    perform(request)
                }
            default:
                break
            }
        }
    #endif
}

#if DEBUG
    private var documentsDirectory: URL {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
    }

    func logHostEvent(_ event: String) {
        let file = documentsDirectory.appendingPathComponent("host.log")
        let line = Data((event + "\n").utf8)
        do {
            if FileManager.default.fileExists(atPath: file.path) {
                let handle = try FileHandle(forWritingTo: file)
                defer { try? handle.close() }
                try handle.seekToEnd()
                try handle.write(contentsOf: line)
            } else {
                try line.write(to: file)
            }
        } catch {
            print("beryl: could not write host.log: \(error)")
        }
    }
#endif
