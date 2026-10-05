import AppKit
import SwiftUI
import TrackerClient

struct TaskIndicatorView: View {
    let taskID: String
    let isRunning: Bool

    var body: some View {
        Image(systemName: isRunning ? "circle.fill" : "circle")
            .font(.system(size: 12, weight: .semibold))
            .foregroundStyle(Color(nsColor: TaskColor.forTaskID(taskID).nativeColor))
            .frame(width: 16)
            .accessibilityLabel(isRunning ? "Tracking" : "Not tracking")
    }
}

extension TaskColor {
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
