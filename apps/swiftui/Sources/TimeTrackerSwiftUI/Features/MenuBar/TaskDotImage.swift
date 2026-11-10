import AppKit
import TrackerClient

/// Draws the colored task dot the status item and the menu's task rows show: filled while tracking, a ring otherwise.
enum TaskDotImage {
    static func make(color: TaskColor?, isRunning: Bool, appearance: NSAppearance) -> NSImage {
        var resolvedColor = color?.nativeColor ?? .secondaryLabelColor
        appearance.performAsCurrentDrawingAppearance {
            resolvedColor = resolvedColor.usingColorSpace(.deviceRGB) ?? resolvedColor
        }
        let drawingColor = resolvedColor
        let image = NSImage(size: NSSize(width: 16, height: 16), flipped: false) { rect in
            let circle = NSBezierPath(ovalIn: rect.insetBy(dx: 3, dy: 3))
            if isRunning {
                drawingColor.setFill()
                circle.fill()
            } else {
                drawingColor.setStroke()
                circle.lineWidth = 1.8
                circle.stroke()
            }
            return true
        }
        image.isTemplate = false
        return image
    }
}

private extension TaskColor {
    var nativeColor: NSColor {
        switch self {
        case .blue: return .systemBlue
        case .teal: return .systemTeal
        case .green: return .systemGreen
        case .orange: return .systemOrange
        case .red: return .systemRed
        case .purple: return .systemPurple
        case .pink: return .systemPink
        }
    }
}
