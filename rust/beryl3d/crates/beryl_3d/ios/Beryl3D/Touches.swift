// Touches for the Rust library: the view that holds the CAMetalLayer, and the pointer ids its
// touches are given.

import QuartzCore
import UIKit

/// A view whose layer is the CAMetalLayer the library draws in. It hands each touch, with its
/// id and phase, to `onTouch`.
final class MetalView: UIView {
    override class var layerClass: AnyClass { CAMetalLayer.self }

    /// Called with a touch's pointer id, its `BERYL_PHASE_` and its position in points.
    var onTouch: ((UInt32, UInt32, CGPoint) -> Void)?
    private var ids = TouchIds()

    override init(frame: CGRect) {
        super.init(frame: frame)
        isMultipleTouchEnabled = true
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        isMultipleTouchEnabled = true
    }

    // One event per touch per callback: coalesced touches are not sent.
    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            if let id = ids.begin(touch) { send(touch, id, BERYL_PHASE_DOWN) }
        }
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            if let id = ids.id(of: touch) { send(touch, id, BERYL_PHASE_MOVE) }
        }
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            if let id = ids.end(touch) { send(touch, id, BERYL_PHASE_UP) }
        }
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            if let id = ids.end(touch) { send(touch, id, BERYL_PHASE_CANCEL) }
        }
    }

    private func send(_ touch: UITouch, _ id: UInt32, _ phase: Int) {
        onTouch?(id, UInt32(phase), touch.location(in: self))
    }
}

/// The pointer id of each touch that is down: the lowest free id from 1 to 255 when it begins
/// (0 is the mouse's), kept until it ends or is cancelled.
struct TouchIds {
    private var ids: [ObjectIdentifier: UInt32] = [:]

    /// Gives `touch` the lowest free id, or nil when all 255 are taken.
    mutating func begin(_ touch: UITouch) -> UInt32? {
        let taken = Set(ids.values)
        guard let id = (1...255).first(where: { !taken.contains(UInt32($0)) }) else {
            return nil
        }
        ids[ObjectIdentifier(touch)] = UInt32(id)
        return UInt32(id)
    }

    func id(of touch: UITouch) -> UInt32? {
        ids[ObjectIdentifier(touch)]
    }

    /// Frees the id of `touch` and returns it.
    mutating func end(_ touch: UITouch) -> UInt32? {
        ids.removeValue(forKey: ObjectIdentifier(touch))
    }
}
