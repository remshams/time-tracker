import Foundation
#if canImport(CoreGraphics)
import CoreGraphics
#endif

public enum TrackerStatusItemHitTest {
    public static let imageSide: CGFloat = 16
    public static let dotInset: CGFloat = 3

    public static func containsDot(_ point: CGPoint, in imageFrame: CGRect) -> Bool {
        guard imageFrame.width > 0, imageFrame.height > 0 else { return false }
        let radiusFraction = (imageSide / 2 - dotInset) / imageSide
        let x = (point.x - imageFrame.midX) / (imageFrame.width * radiusFraction)
        let y = (point.y - imageFrame.midY) / (imageFrame.height * radiusFraction)
        return x * x + y * y <= 1
    }
}
