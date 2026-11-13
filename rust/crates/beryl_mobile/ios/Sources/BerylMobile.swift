import Tauri
import UIKit
import WebKit

class BerylMobilePlugin: Plugin {
    private weak var webview: WKWebView?

    @objc override func load(webview: WKWebView) {
        self.webview = webview
    }

    @objc private func snapshot(_ invoke: Invoke) {
        DispatchQueue.main.async {
            guard let webview = self.webview else {
                invoke.reject("The WebView is unavailable")
                return
            }
            webview.takeSnapshot(with: nil) { image, error in
                guard let png = image?.pngData() else {
                    invoke.reject(error?.localizedDescription ?? "The WebView made no PNG")
                    return
                }
                do {
                    let cache = try FileManager.default.url(
                        for: .cachesDirectory, in: .userDomainMask, appropriateFor: nil, create: true
                    )
                    let file = cache.appendingPathComponent("snapshot-\(UUID().uuidString).png")
                    try png.write(to: file, options: .atomic)
                    invoke.resolve(["path": file.path])
                } catch {
                    invoke.reject(error.localizedDescription)
                }
            }
        }
    }

    @objc private func device(_ invoke: Invoke) {
        DispatchQueue.main.async {
            #if targetEnvironment(simulator)
                let simulator = true
                let model = ProcessInfo.processInfo.environment["SIMULATOR_MODEL_IDENTIFIER"]
                    ?? UIDevice.current.model
            #else
                let simulator = false
                var system = utsname()
                uname(&system)
                let model = withUnsafeBytes(of: &system.machine) {
                    String(decoding: $0.prefix { $0 != 0 }, as: UTF8.self)
                }
            #endif
            invoke.resolve(["os": UIDevice.current.systemVersion, "model": model, "simulator": simulator])
        }
    }
}

@_cdecl("init_plugin_beryl_mobile")
func initPlugin() -> Plugin {
    BerylMobilePlugin()
}
