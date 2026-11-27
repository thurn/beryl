// The view controller of The client's iOS host: one MetalView and a CADisplayLink that asks the
// Rust library for a frame on every tick while the library wants frames.

import CoreFoundation
import QuartzCore
import UIKit

final class TableController: UIViewController {
    private var app: OpaquePointer?
    private var link: CADisplayLink?
    private var foregrounded = true
    private var active = false
    /// The frame rate last asked of the display link, from the library's wake.
    private var hz: Float = 0
    /// The one-shot timer that unpauses the display link at the library's wake time.
    private var timer: DispatchSourceTimer?
    /// The surface last passed, so that a layout that changes nothing passes nothing.
    private var passed: (width: UInt32, height: UInt32, scale: Float, insets: UIEdgeInsets)?
    /// The native field behind the field being edited, which shows the keyboard.
    let textEntry = TextEntry()
    /// The height in points of the keyboard over the view's bottom, or 0 while it is down. The
    /// library is told it as the bottom of the safe area, so that it lays the edited field out
    /// above the keyboard.
    private var keyboard: CGFloat = 0

    // The table fills the screen. Info.plist's UIStatusBarHidden alone no longer hides it.
    override var prefersStatusBarHidden: Bool { true }

    override func loadView() {
        let view = MetalView()
        view.onTouch = { [weak self] id, phase, point in self?.touch(id, phase, point) }
        view.addSubview(textEntry.view)
        textEntry.onText = { [weak self] text in self?.text(text) }
        textEntry.onEnter = { [weak self] in self?.enter() }
        self.view = view
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        let environment = ProcessInfo.processInfo.environment
        // The recording to replay, which `just client ios-smoke` and `ios-device` name. With
        // none, the app plays a live game.
        let fixture = environment["BERYL_CLIENT_FIXTURE"]
        // Debug builds only: the agent that answers the player's decisions in an unattended
        // run, with optional deck, opponent and seed.
        #if DEBUG
            let standIn = environment["BERYL_CLIENT_STAND_IN"]
            let standInDeck = environment["BERYL_CLIENT_STAND_IN_DECK"]
            let standInOpponent = environment["BERYL_CLIENT_STAND_IN_OPPONENT"]
            let standInSeed = environment["BERYL_CLIENT_STAND_IN_SEED"].flatMap(UInt64.init) ?? 0
        #else
            let standIn: String? = nil
            let standInDeck: String? = nil
            let standInOpponent: String? = nil
            let standInSeed: UInt64 = 0
        #endif
        let documents = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        let context = Unmanaged.passUnretained(self).toOpaque()
        app = withOptionalCString(fixture) { fixture in
            withOptionalCString(standIn) { standIn in
                withOptionalCString(standInDeck) { standInDeck in
                    withOptionalCString(standInOpponent) { standInOpponent in
                        documents.path.withCString { dir in
                            var config = BerylConfig(
                                fixture: fixture, data_dir: dir, stand_in: standIn,
                                stand_in_deck: standInDeck, stand_in_opponent: standInOpponent,
                                stand_in_seed: standInSeed,
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
                            return beryl_client_new(&config)
                        }
                    }
                }
            }
        }
        let link = CADisplayLink(target: self, selector: #selector(tick))
        active = UIApplication.shared.applicationState == .active
        link.isPaused = !active
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
        center.addObserver(
            self, selector: #selector(resignActive),
            name: UIApplication.willResignActiveNotification, object: nil)
        center.addObserver(
            self, selector: #selector(becomeActive),
            name: UIApplication.didBecomeActiveNotification, object: nil)
        center.addObserver(
            self, selector: #selector(keyboardChanged),
            name: UIResponder.keyboardWillChangeFrameNotification, object: nil)
        center.addObserver(
            self, selector: #selector(keyboardChanged),
            name: UIResponder.keyboardWillHideNotification, object: nil)
        #if DEBUG
            observeTestNotifications(context: context)
        #endif
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
        guard let app, foregrounded else { return }
        let layer = view.layer as! CAMetalLayer
        let scale = view.window?.screen.nativeScale ?? UIScreen.main.nativeScale
        layer.contentsScale = scale
        var insets = view.safeAreaInsets
        insets.bottom = max(insets.bottom, keyboard)
        let width = UInt32(view.bounds.width * scale)
        let height = UInt32(view.bounds.height * scale)
        if let passed, passed.width == width, passed.height == height,
            passed.scale == Float(scale), passed.insets == insets
        {
            return
        }
        passed = (width, height, Float(scale), insets)
        resume()
        beryl_client_surface(
            app, Unmanaged.passUnretained(layer).toOpaque(), width, height, Float(scale),
            BerylInsets(
                top: Float(insets.top), right: Float(insets.right),
                bottom: Float(insets.bottom), left: Float(insets.left)))
    }

    private func touch(_ id: UInt32, _ phase: UInt32, _ point: CGPoint) {
        guard let app else { return }
        resume()
        beryl_client_pointer(
            app,
            BerylPointer(
                id: id, kind: UInt32(BERYL_POINTER_TOUCH), phase: phase, x: Float(point.x),
                y: Float(point.y), button: UInt32(BERYL_BUTTON_PRIMARY), wheel_x: 0, wheel_y: 0,
                modifiers: 0))
    }

    /// The edited field's whole new contents, typed on the keyboard.
    private func text(_ text: String) {
        guard let app else { return }
        resume()
        text.withCString { beryl_client_text(app, $0) }
    }

    /// Return on the keyboard, as the Enter key's press and release.
    private func enter() {
        guard let app else { return }
        resume()
        for phase in [BERYL_KEY_DOWN, BERYL_KEY_UP] {
            beryl_client_key(
                app, BerylKey(code: UInt32(BERYL_KEY_ENTER), phase: UInt32(phase), modifiers: 0))
        }
    }

    @objc private func keyboardChanged(_ notification: Notification) {
        let height =
            notification.name == UIResponder.keyboardWillHideNotification
            ? 0 : TableController.keyboardHeight(notification, in: view)
        guard height != keyboard else { return }
        keyboard = height
        attach()
    }

    @objc private func tick(_ link: CADisplayLink) {
        guard let app, foregrounded, active else {
            link.isPaused = true
            return
        }
        let now = UInt64(CACurrentMediaTime() * 1e9)
        let wake = beryl_client_frame(app, now)
        var request = BerylRequest()
        while beryl_client_next_request(app, &request) {
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
        guard foregrounded, active, let link else { return }
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

    /// A wake from Rust or a scheduled timer can arrive while the app is inactive.
    func resume() {
        guard foregrounded, active else { return }
        timer?.cancel()
        timer = nil
        link?.isPaused = false
    }

    private func pause() {
        timer?.cancel()
        timer = nil
        link?.isPaused = true
    }

    @objc private func resignActive() {
        active = false
        pause()
    }

    @objc private func becomeActive() {
        active = true
        resume()
    }

    @objc private func background() {
        foregrounded = false
        passed = nil
        if let app {
            beryl_client_lifecycle(app, UInt32(BERYL_SUSPENDED))
            beryl_client_surface_lost(app)
            #if DEBUG
                logHostEvent("surface-lost")
            #endif
        }
        pause()
    }

    @objc private func foreground() {
        foregrounded = true
        guard let app else { return }
        beryl_client_lifecycle(app, UInt32(BERYL_RESUMED))
        attach()
        resume()
    }

    @objc private func lowMemory() {
        guard let app else { return }
        beryl_client_lifecycle(app, UInt32(BERYL_LOW_MEMORY))
        resume()
    }

    #if DEBUG
        // The simulator cannot raise a UIKit memory warning through simctl. These notifications
        // let the headless smoke drive the same handlers as UIKit and the Rust request queue.
        private func observeTestNotifications(context: UnsafeMutableRawPointer) {
            let center = CFNotificationCenterGetDarwinNotifyCenter()
            for name in ["memory-warning", "share", "haptic", "open-url"] {
                CFNotificationCenterAddObserver(
                    center, context,
                    { _, observer, name, _, _ in
                        guard let observer, let name else { return }
                        let controller = Unmanaged<TableController>.fromOpaque(observer)
                            .takeUnretainedValue()
                        let event = String((name.rawValue as String).split(separator: ".").last ?? "")
                        DispatchQueue.main.async {
                            if event == "memory-warning" {
                                controller.lowMemory()
                            } else {
                                controller.simulateRequest(event)
                            }
                        }
                    },
                    "ca.thurn.beryl.test.\(name)" as CFString, nil, .deliverImmediately)
            }
        }
    #endif
}

/// Calls `body` with `string` as a C string, or with nil when there is none.
private func withOptionalCString<Result>(
    _ string: String?, _ body: (UnsafePointer<CChar>?) -> Result
) -> Result {
    guard let string else { return body(nil) }
    return string.withCString { body($0) }
}
