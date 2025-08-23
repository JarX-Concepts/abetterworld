//
//  ABWGestureSink.swift
//  ABetterWorld
//
//  Created by Andrew Tosh on 8/20/25.
//

import UIKit

@MainActor
public protocol ABWGestureSink: AnyObject {
    func orbit(begin: Bool, dx: Double, dy: Double, vx: Double, vy: Double)
    func translate(begin: Bool, dx: Double, dy: Double, vx: Double, vy: Double)
    func zoom(begin: Bool, scale: Double, velocity: Double)
    func rotate(begin: Bool, radians: Double, velocity: Double)
    func doubleTap(x: Double, y: Double)
    func touchDown(active: Bool, x: Double, y: Double) // optional “pointer down”
}

/// Owns recognizers and maps them to ABWGestureSink calls.
@MainActor
public final class ABWGestureController: NSObject, UIGestureRecognizerDelegate {
    private weak var view: UIView?
    private weak var sink: ABWGestureSink?
    private let scaleProvider: () -> CGFloat

    // Keep strong refs so they aren’t GC’d
    private var recognizers: [UIGestureRecognizer] = []

    public init(attachingTo view: UIView, sink: ABWGestureSink, scaleProvider: @escaping () -> CGFloat) {
        self.view = view
        self.sink = sink
        self.scaleProvider = scaleProvider
        super.init()
        install(on: view)
    }

    public func install(on view: UIView) {
        let orbit = UIPanGestureRecognizer(target: self, action: #selector(handleOrbit(_:)))
        orbit.minimumNumberOfTouches = 1; orbit.maximumNumberOfTouches = 1
        orbit.cancelsTouchesInView = false
        orbit.delegate = self

        let translate = UIPanGestureRecognizer(target: self, action: #selector(handleTranslate(_:)))
        translate.minimumNumberOfTouches = 2; translate.maximumNumberOfTouches = 2
        translate.cancelsTouchesInView = false
        translate.delegate = self

        let pinch = UIPinchGestureRecognizer(target: self, action: #selector(handlePinch(_:)))
        pinch.cancelsTouchesInView = false
        pinch.delegate = self

        let rotate = UIRotationGestureRecognizer(target: self, action: #selector(handleRotate(_:)))
        rotate.cancelsTouchesInView = false
        rotate.delegate = self

        let doubleTap = UITapGestureRecognizer(target: self, action: #selector(handleDoubleTap(_:)))
        doubleTap.numberOfTapsRequired = 2

        // Optional: “touch down” without raw touches
        let touchDown = UILongPressGestureRecognizer(target: self, action: #selector(handleTouchDown(_:)))
        touchDown.minimumPressDuration = 0
        touchDown.allowableMovement = .greatestFiniteMagnitude
        touchDown.cancelsTouchesInView = false

        recognizers = [orbit, translate, pinch, rotate, doubleTap, touchDown]
        recognizers.forEach { view.addGestureRecognizer($0) }
    }

    func remove() {
        guard let v = view else { return }
        recognizers.forEach { v.removeGestureRecognizer($0) }
        recognizers.removeAll()
    }

    // Allow combos like pinch+rotate
    public func gestureRecognizer(_ g: UIGestureRecognizer,
                                  shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool { true }

    // MARK: Handlers
    @objc private func handleOrbit(_ g: UIPanGestureRecognizer) {
        guard let sink else { return }
        let s = scaleProvider()
        let d = g.translation(in: g.view)
        let v = g.velocity(in: g.view)
        switch g.state {
        case .began:
            sink.orbit(begin: true,  dx: 0,                dy: 0,                vx: 0,             vy: 0)
        case .changed:
            sink.orbit(begin: false, dx: Double(d.x*s),    dy: Double(d.y*s),    vx: Double(v.x*s), vy: Double(v.y*s))
        case .ended, .cancelled, .failed:
            sink.orbit(begin: false, dx: 0,                dy: 0,                vx: Double(v.x*s), vy: Double(v.y*s))
            g.setTranslation(.zero, in: g.view)
        default: break
        }
    }

    @objc private func handleTranslate(_ g: UIPanGestureRecognizer) {
        guard let sink else { return }
        let s = scaleProvider()
        let d = g.translation(in: g.view)
        let v = g.velocity(in: g.view)
        switch g.state {
        case .began:
            sink.translate(begin: true,  dx: 0,               dy: 0,               vx: 0,             vy: 0)
        case .changed:
            sink.translate(begin: false, dx: Double(d.x*s),   dy: Double(d.y*s),   vx: Double(v.x*s), vy: Double(v.y*s))
        case .ended, .cancelled, .failed:
            sink.translate(begin: false, dx: 0,               dy: 0,               vx: Double(v.x*s), vy: Double(v.y*s))
            g.setTranslation(.zero, in: g.view)
        default: break
        }
    }

    @objc private func handlePinch(_ g: UIPinchGestureRecognizer) {
        guard let sink else { return }
        switch g.state {
        case .began: sink.zoom(begin: true,  scale: 1.0,               velocity: 0)
        case .changed: sink.zoom(begin: false, scale: Double(g.scale), velocity: Double(g.velocity))
        case .ended, .cancelled, .failed: sink.zoom(begin: false, scale: Double(g.scale), velocity: Double(g.velocity))
        default: break
        }
    }

    @objc private func handleRotate(_ g: UIRotationGestureRecognizer) {
        guard let sink else { return }
        switch g.state {
        case .began: sink.rotate(begin: true,  radians: 0,                   velocity: 0)
        case .changed: sink.rotate(begin: false, radians: Double(g.rotation), velocity: Double(g.velocity))
        case .ended, .cancelled, .failed: sink.rotate(begin: false, radians: Double(g.rotation), velocity: Double(g.velocity))
        default: break
        }
    }

    @objc private func handleDoubleTap(_ g: UITapGestureRecognizer) {
        guard let sink, let v = g.view else { return }
        let s = scaleProvider()
        let p = g.location(in: v)
        sink.doubleTap(x: Double(p.x*s), y: Double(p.y*s))
    }

    @objc private func handleTouchDown(_ g: UILongPressGestureRecognizer) {
        guard let sink, let v = g.view else { return }
        let s = scaleProvider()
        let p = g.location(in: v)
        switch g.state {
        case .began: sink.touchDown(active: true,  x: Double(p.x*s), y: Double(p.y*s))
        case .changed: sink.touchDown(active: true,  x: Double(p.x*s), y: Double(p.y*s))
        case .ended, .cancelled, .failed: sink.touchDown(active: false, x: Double(p.x*s), y: Double(p.y*s))
        default: break
        }
    }
}

typealias ABWHandle = UnsafeMutablePointer<ABetterWorldiOS>

final class ABWGestureAdapter: ABWGestureSink {
    private unowned let owner: ABetterWorldRenderer
    init(owner: ABetterWorldRenderer) { self.owner = owner }

    private var ptr: ABWHandle { owner.getRenderer()! }

    func orbit(begin: Bool, dx: Double, dy: Double, vx: Double, vy: Double) {
        
        abetterworld_ios_resize(ptr, 50, 50)
        
        abetterworld_ios_gesture_pan_orbit(ptr, begin, dx, dy, vx, vy)
    }
    func translate(begin: Bool, dx: Double, dy: Double, vx: Double, vy: Double) {
        abetterworld_ios_gesture_pan_translate(ptr, begin, dx, dy, vx, vy)
    }
    func zoom(begin: Bool, scale: Double, velocity: Double) {
        abetterworld_ios_gesture_pinch(ptr, begin, scale, velocity)
    }
    func rotate(begin: Bool, radians: Double, velocity: Double) {
        abetterworld_ios_gesture_rotate(ptr, begin, radians, velocity)
    }
    func doubleTap(x: Double, y: Double) {
        abetterworld_ios_gesture_double_tap(ptr, x, y)
    }
    func touchDown(active: Bool, x: Double, y: Double) {
        abetterworld_ios_touch_down(ptr, active, x, y)
    }
}
