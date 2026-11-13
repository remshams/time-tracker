import SwiftUI

/// Places subviews left to right and wraps to a new line when the next one doesn't fit.
struct FlowLayout: Layout {
    var spacing: CGFloat = 8
    var lineSpacing: CGFloat = 8

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let lines = arrange(width: proposal.width ?? .infinity, subviews: subviews)
        return CGSize(
            width: proposal.width ?? lines.map(\.width).max() ?? 0,
            height: lines.reduce(0) { $0 + $1.height } + lineSpacing * CGFloat(max(0, lines.count - 1)))
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var y = bounds.minY
        for line in arrange(width: bounds.width, subviews: subviews) {
            var x = bounds.minX
            for item in line.items {
                subviews[item.index].place(
                    at: CGPoint(x: x, y: y), anchor: .topLeading, proposal: ProposedViewSize(item.size))
                x += item.size.width + spacing
            }
            y += line.height + lineSpacing
        }
    }

    private struct Line {
        var items: [(index: Int, size: CGSize)] = []
        var width: CGFloat = 0
        var height: CGFloat = 0
    }

    private func arrange(width: CGFloat, subviews: Subviews) -> [Line] {
        var lines = [Line()]
        for index in subviews.indices {
            var size = subviews[index].sizeThatFits(.unspecified)
            size.width = min(size.width, width)
            let needed = (lines[lines.count - 1].items.isEmpty ? 0 : spacing) + size.width
            if lines[lines.count - 1].width + needed > width, !lines[lines.count - 1].items.isEmpty {
                lines.append(Line())
            }
            let spacingBefore = lines[lines.count - 1].items.isEmpty ? 0 : spacing
            lines[lines.count - 1].items.append((index, size))
            lines[lines.count - 1].width += spacingBefore + size.width
            lines[lines.count - 1].height = max(lines[lines.count - 1].height, size.height)
        }
        return lines
    }
}
