// The iOS host of Beryl 3D's phone spike (docs/beryl-3d.md, "Hosts"): one view backed by a
// CAMetalLayer, and a CADisplayLink that asks the Rust library for a frame on every tick. It
// passes the layer, the app's lifecycle and the display's time, and nothing else: everything
// the app does is in the library's C functions (include/beryl3d.h).

import QuartzCore
import UIKit

@main
final class AppDelegate: UIResponder, UIApplicationDelegate {
    var window: UIWindow?

    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
    ) -> Bool {
        let window = UIWindow(frame: UIScreen.main.bounds)
        window.rootViewController = TableController()
        window.makeKeyAndVisible()
        self.window = window
        return true
    }
}

/// A view whose layer is the CAMetalLayer the library draws in.
final class MetalView: UIView {
    override class var layerClass: AnyClass { CAMetalLayer.self }
}

final class TableController: UIViewController {
    private var app: OpaquePointer?
    private var link: CADisplayLink?
    private var visible = true

    override func loadView() {
        view = MetalView()
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        // The recording to replay: `just b3d ios-smoke` and `ios-device` name it.
        let fixture = ProcessInfo.processInfo.environment["BERYL3D_FIXTURE"] ?? "verbs-zones"
        let documents = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        app = fixture.withCString { fixture in
            documents.path.withCString { dir in
                var config = BerylConfig(
                    fixture: fixture, data_dir: dir, stand_in: nil,
                    display_hz: Float(UIScreen.main.maximumFramesPerSecond), wake: nil,
                    context: nil)
                return beryl3d_new(&config)
            }
        }
        let link = CADisplayLink(target: self, selector: #selector(tick))
        // The library counts frames against a fixed 60 Hz until its rate rule drives this
        // link (beryl_host's `Phone`). A range from 60 up to the display's highest would let a
        // ProMotion display show frames at uneven rates that the frame log never calls late.
        link.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 60, preferred: 60)
        link.add(to: .main, forMode: .common)
        self.link = link
        let center = NotificationCenter.default
        center.addObserver(
            self, selector: #selector(background),
            name: UIApplication.didEnterBackgroundNotification, object: nil)
        center.addObserver(
            self, selector: #selector(foreground),
            name: UIApplication.willEnterForegroundNotification, object: nil)
        center.addObserver(
            self, selector: #selector(lowMemory),
            name: UIApplication.didReceiveMemoryWarningNotification, object: nil)
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        attach()
    }

    /// Passes the layer at the view's size in pixels.
    private func attach() {
        guard let app, visible else { return }
        let layer = view.layer as! CAMetalLayer
        let scale = view.window?.screen.nativeScale ?? UIScreen.main.nativeScale
        layer.contentsScale = scale
        let insets = view.safeAreaInsets
        beryl3d_surface(
            app, Unmanaged.passUnretained(layer).toOpaque(),
            UInt32(view.bounds.width * scale), UInt32(view.bounds.height * scale), Float(scale),
            BerylInsets(
                top: Float(insets.top), right: Float(insets.right),
                bottom: Float(insets.bottom), left: Float(insets.left)))
    }

    @objc private func tick(_ link: CADisplayLink) {
        guard let app, visible else { return }
        _ = beryl3d_frame(app, UInt64(CACurrentMediaTime() * 1e9))
    }

    @objc private func background() {
        guard let app else { return }
        visible = false
        beryl3d_lifecycle(app, UInt32(BERYL_SUSPENDED))
        beryl3d_surface_lost(app)
    }

    @objc private func foreground() {
        guard let app else { return }
        visible = true
        beryl3d_lifecycle(app, UInt32(BERYL_RESUMED))
        attach()
    }

    @objc private func lowMemory() {
        guard let app else { return }
        beryl3d_lifecycle(app, UInt32(BERYL_LOW_MEMORY))
    }
}
