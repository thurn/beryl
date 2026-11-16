// The view controller of Beryl 3D's iOS host: one MetalView and a CADisplayLink that asks the
// Rust library for a frame on every tick while the library wants frames.

import QuartzCore
import UIKit

final class TableController: UIViewController {
    private var app: OpaquePointer?
    private var link: CADisplayLink?
    private var visible = true
    /// The frame rate last asked of the display link, from the library's wake.
    private var hz: Float = 0
    /// The one-shot timer that unpauses the display link at the library's wake time.
    private var timer: DispatchSourceTimer?
    /// The surface last passed, so that a layout that changes nothing passes nothing.
    private var passed: (width: UInt32, height: UInt32, scale: Float, insets: UIEdgeInsets)?

    // The table fills the screen. Info.plist's UIStatusBarHidden alone no longer hides it.
    override var prefersStatusBarHidden: Bool { true }

    override func loadView() {
        let view = MetalView()
        view.onTouch = { [weak self] id, phase, point in self?.touch(id, phase, point) }
        self.view = view
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        let environment = ProcessInfo.processInfo.environment
        // The recording to replay, which `just b3d ios-smoke` and `ios-device` name. With
        // none, the app plays a live game.
        let fixture = environment["BERYL3D_FIXTURE"]
        // Debug builds only: the agent that answers the player's decisions in an unattended
        // run.
        #if DEBUG
            let standIn = environment["BERYL3D_STAND_IN"]
        #else
            let standIn: String? = nil
        #endif
        let documents = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        let context = Unmanaged.passUnretained(self).toOpaque()
        app = withOptionalCString(fixture) { fixture in
            withOptionalCString(standIn) { standIn in
                documents.path.withCString { dir in
                    var config = BerylConfig(
                        fixture: fixture, data_dir: dir, stand_in: standIn,
                        display_hz: Float(UIScreen.main.maximumFramesPerSecond),
                        wake: { context in
                            // Any thread: ask for a frame soon, on the main thread. The
                            // controller lives as long as the app.
                            guard let context else { return }
                            let controller = Unmanaged<TableController>.fromOpaque(context)
                                .takeUnretainedValue()
                            DispatchQueue.main.async { controller.resume() }
                        },
                        context: context)
                    return beryl3d_new(&config)
                }
            }
        }
        let link = CADisplayLink(target: self, selector: #selector(tick))
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

    override func viewSafeAreaInsetsDidChange() {
        super.viewSafeAreaInsetsDidChange()
        attach()
    }

    /// Passes the layer at the view's size in pixels and its safe area in points. Rotating
    /// passes a new size for the same layer.
    private func attach() {
        guard let app, visible else { return }
        let layer = view.layer as! CAMetalLayer
        let scale = view.window?.screen.nativeScale ?? UIScreen.main.nativeScale
        layer.contentsScale = scale
        let insets = view.safeAreaInsets
        let width = UInt32(view.bounds.width * scale)
        let height = UInt32(view.bounds.height * scale)
        if let passed, passed.width == width, passed.height == height,
            passed.scale == Float(scale), passed.insets == insets
        {
            return
        }
        passed = (width, height, Float(scale), insets)
        resume()
        beryl3d_surface(
            app, Unmanaged.passUnretained(layer).toOpaque(), width, height, Float(scale),
            BerylInsets(
                top: Float(insets.top), right: Float(insets.right),
                bottom: Float(insets.bottom), left: Float(insets.left)))
    }

    private func touch(_ id: UInt32, _ phase: UInt32, _ point: CGPoint) {
        guard let app else { return }
        resume()
        beryl3d_pointer(
            app,
            BerylPointer(
                id: id, kind: UInt32(BERYL_POINTER_TOUCH), phase: phase, x: Float(point.x),
                y: Float(point.y), button: UInt32(BERYL_BUTTON_PRIMARY), wheel_x: 0, wheel_y: 0,
                modifiers: 0))
    }

    @objc private func tick(_ link: CADisplayLink) {
        guard let app, visible else {
            link.isPaused = true
            return
        }
        let now = UInt64(CACurrentMediaTime() * 1e9)
        let wake = beryl3d_frame(app, now)
        var request = BerylRequest()
        while beryl3d_next_request(app, &request) {
            perform(request)
        }
        if wake.hz > 0, wake.hz != hz {
            hz = wake.hz
            link.preferredFrameRateRange = CAFrameRateRange(
                minimum: wake.hz, maximum: wake.hz, preferred: wake.hz)
        }
        schedule(wake, now: now)
    }

    /// Pauses the display link until the library's next wake: for good when it wants no
    /// frame, or until a timer at its time. A time within the next tick keeps the link running,
    /// since a timer could not wake it sooner.
    private func schedule(_ wake: BerylWake, now: UInt64) {
        guard let link else { return }
        switch Int(wake.kind) {
        case BERYL_WAKE_NEVER:
            link.isPaused = true
        case BERYL_WAKE_AT:
            let interval = UInt64(1e9 / Double(max(hz, 1)))
            guard wake.at > now + interval else { return }
            link.isPaused = true
            let timer = DispatchSource.makeTimerSource(queue: .main)
            timer.schedule(deadline: .now() + .nanoseconds(Int(wake.at - now)))
            timer.setEventHandler { [weak self] in self?.resume() }
            timer.resume()
            self.timer = timer
        default:
            break
        }
    }

    /// Unpauses the display link, so that the next tick asks the library for a frame.
    func resume() {
        timer?.cancel()
        timer = nil
        link?.isPaused = false
    }

    /// Carries out something the library asks of the platform.
    private func perform(_ request: BerylRequest) {
        switch Int(request.kind) {
        case BERYL_REQUEST_BEGIN_TEXT_ENTRY:
            log(request, "begin text entry")
        case BERYL_REQUEST_END_TEXT_ENTRY:
            log(request, "end text entry")
        case BERYL_REQUEST_SHARE:
            log(request, "share")
        case BERYL_REQUEST_HAPTIC:
            log(request, "haptic")
        case BERYL_REQUEST_OPEN_URL:
            log(request, "open URL")
        default:
            log(request, "unknown")
        }
    }

    private func log(_ request: BerylRequest, _ kind: String) {
        #if DEBUG
            print("beryl3d: request \(kind) (\(request.kind))")
        #endif
    }

    @objc private func background() {
        guard let app else { return }
        resume()
        visible = false
        passed = nil
        beryl3d_lifecycle(app, UInt32(BERYL_SUSPENDED))
        beryl3d_surface_lost(app)
    }

    @objc private func foreground() {
        guard let app else { return }
        resume()
        visible = true
        beryl3d_lifecycle(app, UInt32(BERYL_RESUMED))
        attach()
    }

    @objc private func lowMemory() {
        guard let app else { return }
        resume()
        beryl3d_lifecycle(app, UInt32(BERYL_LOW_MEMORY))
    }
}

/// Calls `body` with `string` as a C string, or with nil when there is none.
private func withOptionalCString<Result>(
    _ string: String?, _ body: (UnsafePointer<CChar>?) -> Result
) -> Result {
    guard let string else { return body(nil) }
    return string.withCString { body($0) }
}
