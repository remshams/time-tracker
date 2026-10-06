import Foundation

public enum TrackerMenuPanelPlacement {
    public static func frame(contentSize: CGSize, anchor: CGRect, visibleFrame: CGRect,
                             gap: CGFloat = 6) -> CGRect {
        let width = min(max(0, contentSize.width), visibleFrame.width)
        let top = min(max(anchor.minY - gap, visibleFrame.minY), visibleFrame.maxY)
        let height = min(max(0, contentSize.height), top - visibleFrame.minY)
        let x = min(max(anchor.maxX - width, visibleFrame.minX), visibleFrame.maxX - width)
        return CGRect(x: x, y: top - height, width: width, height: height)
    }
}
