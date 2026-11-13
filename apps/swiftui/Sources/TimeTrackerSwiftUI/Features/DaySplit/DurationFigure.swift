import SwiftUI

/// A duration in large rounded figures. Hours and minutes carry the size and the seconds sit beside them, smaller.
struct DurationFigure: View {
    let text: String
    let size: CGFloat
    var weight: Font.Weight = .semibold
    @ScaledMetric(relativeTo: .largeTitle) private var scale = 1

    var body: some View {
        let parts = text.split(separator: ":")
        Group {
            if parts.count == 3 {
                HStack(alignment: .firstTextBaseline, spacing: 0) {
                    Text("\(parts[0]):\(parts[1])")
                        .font(.system(size: size * scale, weight: weight, design: .rounded))
                    Text(":\(parts[2])")
                        .font(.system(size: size * scale * 0.5, weight: .medium, design: .rounded))
                        .opacity(0.55)
                }
            } else {
                Text(text)
                    .font(.system(size: size * scale * 0.5, weight: .medium, design: .rounded))
            }
        }
        .monospacedDigit()
        .lineLimit(1)
        .minimumScaleFactor(0.6)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(text)
    }
}
