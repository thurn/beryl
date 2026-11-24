// The UI test that `just client ios-smoke --touch` runs: it touches, drags, taps with two fingers
// and rotates the app, then leaves it idle and sends it to the background. The command then
// reads the app's frame log to prove each event reached the Rust library and that the host
// stopped asking for frames while idle.

import XCTest

final class Touches: XCTestCase {
    func testTouchesRotationAndIdle() {
        let app = XCUIApplication()
        // A live game: it waits on the player's first decision, so nothing moves once the
        // table has settled, and its frame log stays open after the inputs.
        app.launch()
        sleep(2)
        let origin = app.coordinate(withNormalizedOffset: CGVector(dx: 0, dy: 0))
        origin.withOffset(CGVector(dx: 100, dy: 100)).tap()
        // 100 points in half a second.
        origin.withOffset(CGVector(dx: 100, dy: 300)).press(
            forDuration: 0.05, thenDragTo: origin.withOffset(CGVector(dx: 200, dy: 300)),
            withVelocity: XCUIGestureVelocity(200), thenHoldForDuration: 0)
        app.twoFingerTap()
        XCUIDevice.shared.orientation = .landscapeLeft
        sleep(2)
        XCUIDevice.shared.orientation = .portrait
        // Long enough for the table to settle, then 2 seconds with nothing moving, which the
        // command checks from the time since the last frame when the app is suspended.
        sleep(6)
        XCUIDevice.shared.press(.home)
        sleep(2)
    }
}
