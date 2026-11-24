// The iOS host of the client (docs/beryl-3d.md, "Hosts"): the app delegate, which sets up the
// audio session and the one window. The view controller (Table.swift) passes the layer, the
// touches (Touches.swift), the safe area, the app's lifecycle and the display's time to the
// Rust library, and carries out the frame rate and wake times it returns. Every decision about
// the game or its layout is the library's (include/beryl_client.h).

import AVFoundation
import UIKit

@main
final class AppDelegate: UIResponder, UIApplicationDelegate {
    var window: UIWindow?

    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
    ) -> Bool {
        // Ambient: Beryl's sounds mix with the player's music and respect the silent switch.
        do {
            let session = AVAudioSession.sharedInstance()
            try session.setCategory(.ambient)
            try session.setActive(true)
        } catch {
            print("beryl: the audio session could not be set up: \(error)")
        }
        let window = UIWindow(frame: UIScreen.main.bounds)
        window.rootViewController = TableController()
        window.makeKeyAndVisible()
        self.window = window
        return true
    }
}
