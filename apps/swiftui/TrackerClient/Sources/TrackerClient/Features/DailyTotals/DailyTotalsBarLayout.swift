import Foundation

/// The widths of today's bar segments and the gap between them, in points.
///
/// Each segment is as wide as its share, but at least `minimum`, so a task with a few seconds stays visible and
/// clickable. Raising a share to the minimum takes width from the wider ones. When the minimum widths and gaps don't
/// fit, the gaps go and the minimum shrinks, so every segment stays inside the bar.
public struct DailyTotalsBarLayout: Equatable, Sendable {
    public let gap: Double
    /// One width per share, in the order of the shares.
    public let widths: [Double]

    public init(fractions: [Double], width: Double, gap: Double, minimum: Double) {
        guard !fractions.isEmpty else {
            self.gap = gap
            widths = []
            return
        }
        let count = Double(fractions.count)
        let crowded = count * (minimum + gap) > width + gap
        self.gap = crowded ? 0 : gap
        let minimum = min(minimum, max(0, width) / count)
        let usable = max(0, width - self.gap * (count - 1))
        // Each pass that finds a share below the minimum adds at least one, so one pass per share and a last one to
        // confirm are enough.
        var narrow = Set<Int>()
        for _ in 0...fractions.count {
            let tooNarrow = Self.wide(fractions, narrow: narrow, usable: usable, minimum: minimum).filter {
                $0.width < minimum
            }
            if tooNarrow.isEmpty { break }
            narrow.formUnion(tooNarrow.map(\.index))
        }
        let wide = Dictionary(
            uniqueKeysWithValues: Self.wide(fractions, narrow: narrow, usable: usable, minimum: minimum).map {
                ($0.index, $0.width)
            })
        widths = fractions.indices.map { wide[$0] ?? minimum }
    }

    /// The widths of the shares outside `narrow`, which split what the narrow ones leave by their fractions.
    private static func wide(
        _ fractions: [Double], narrow: Set<Int>, usable: Double, minimum: Double
    ) -> [(index: Int, width: Double)] {
        let rest = max(0, usable - minimum * Double(narrow.count))
        let wide = fractions.indices.filter { !narrow.contains($0) }
        let sum = wide.reduce(0) { $0 + fractions[$1] }
        return wide.map { ($0, sum == 0 ? 0 : rest * fractions[$0] / sum) }
    }
}
