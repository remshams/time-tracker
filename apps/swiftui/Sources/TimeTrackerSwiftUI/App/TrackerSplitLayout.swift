import AppKit
import Combine
import SwiftUI

@MainActor
final class TrackerSplitViewController: NSSplitViewController, NSToolbarDelegate {
    private enum ItemID {
        static let sidebar = NSToolbarItem.Identifier("TrackerSidebarControls")
        static let collapse = NSToolbarItem.Identifier("TrackerToggleSidebar")
        static let settings = NSToolbarItem.Identifier("TrackerSettings")
        static let create = NSToolbarItem.Identifier("TrackerCreateTask")
        static let separator = NSToolbarItem.Identifier("TrackerSidebarSeparator")
        static let detail = NSToolbarItem.Identifier("TrackerTaskControls")
        static let rename = NSToolbarItem.Identifier("TrackerRenameTask")
        static let tracking = NSToolbarItem.Identifier("TrackerTracking")
    }

    private struct ToolbarContent: Equatable {
        let title: String
        let hasSelectedTask: Bool
        let isArchived: Bool
        let canCreate: Bool
        let canRename: Bool
        let isRunning: Bool
        let canTrack: Bool
        let hasActiveTimer: Bool
    }

    let store: TrackerStore
    var openSettings: () -> Void
    private let sidebarItem: NSSplitViewItem
    private var subscriptions: Set<AnyCancellable> = []
    private weak var installedWindow: NSWindow?
    private var renderedContent: ToolbarContent?
    private var isTornDown = false
    private var updatingSeparator = false

    private lazy var collapseItem = actionItem(
        ItemID.collapse, label: "Hide sidebar", symbol: "sidebar.left", action: #selector(toggleSidebarAction)
    )
    private lazy var settingsItem = actionItem(
        ItemID.settings, label: "Settings", symbol: "gearshape", action: #selector(showSettings)
    )
    private lazy var creationItem = actionItem(
        ItemID.create, label: "New task", symbol: "plus", action: #selector(createTask)
    )
    private lazy var renameItem = actionItem(
        ItemID.rename, label: "Edit task name", symbol: "pencil", action: #selector(renameTask)
    )
    private lazy var trackingItem = actionItem(
        ItemID.tracking, label: "Start tracking", symbol: "play.fill", action: #selector(changeTracking)
    )
    private lazy var sidebarControls = group(
        ItemID.sidebar, label: "Sidebar", items: [collapseItem, settingsItem, creationItem], priority: .user
    )
    private lazy var detailControls = group(
        ItemID.detail, label: "Task", items: [renameItem, trackingItem], priority: .high
    )
    private lazy var nativeToolbar: NSToolbar = {
        // AppKit synchronizes item changes between toolbars with the same identifier.
        let toolbar = NSToolbar(identifier: NSToolbar.Identifier("TrackerWindow-\(UUID().uuidString)"))
        toolbar.delegate = self
        toolbar.allowsUserCustomization = false
        toolbar.autosavesConfiguration = false
        toolbar.displayMode = .iconOnly
        return toolbar
    }()

    init(store: TrackerStore, presentation: TrackerTaskPresentationCoordinator,
         window: NSWindow, openSettings: @escaping () -> Void) {
        self.store = store
        self.openSettings = openSettings
        let sidebar = TrackerPaneViewController(content: AnyView(
            TrackerSidebar(store: store)
        ))
        let detail = TrackerPaneViewController(content: AnyView(
            TrackerDetailPresentation(store: store, presentation: presentation, windowID: ObjectIdentifier(window))
        ))
        sidebarItem = NSSplitViewItem(sidebarWithViewController: sidebar)
        super.init(nibName: nil, bundle: nil)

        let split = TrackerWindowSplitView()
        split.isVertical = true
        split.dividerStyle = .thin
        split.onWindowChange = { [weak self] window in self?.attach(to: window) }
        splitView = split

        sidebarItem.minimumThickness = 280
        sidebarItem.maximumThickness = 480
        sidebarItem.holdingPriority = .defaultHigh
        sidebarItem.canCollapse = true
        sidebarItem.canCollapseFromWindowResize = false
        let detailItem = NSSplitViewItem(viewController: detail)
        detailItem.minimumThickness = 380
        addSplitViewItem(sidebarItem)
        addSplitViewItem(detailItem)

        let preferredWidth = sidebar.view.widthAnchor.constraint(equalToConstant: 340)
        preferredWidth.priority = .defaultLow
        preferredWidth.isActive = true

        for publisher in [store.objectWillChange, store.activity.objectWillChange,
                          store.creation.objectWillChange, store.rename.objectWillChange] {
            publisher.sink { [weak self] _ in self?.updateToolbarContent() }
                .store(in: &subscriptions)
        }
        NotificationCenter.default.publisher(for: NSSplitView.didResizeSubviewsNotification, object: split)
            .sink { [weak self] _ in self?.updateSidebarSection() }
            .store(in: &subscriptions)
    }

    required init?(coder: NSCoder) {
        fatalError("TrackerSplitViewController requires a TrackerStore")
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        attach(to: view.window)
    }

    private func attach(to window: NSWindow?) {
        guard !isTornDown else { return }
        guard installedWindow !== window else { return }
        detachToolbar()
        guard let window else { return }
        installedWindow = window
        window.toolbar = nativeToolbar
        updateToolbarContent()
        updateSidebarSection()
    }

    private func detachToolbar() {
        if let window = installedWindow, window.toolbar === nativeToolbar { window.toolbar = nil }
        installedWindow = nil
        renderedContent = nil
    }

    func tearDown() {
        isTornDown = true
        subscriptions.removeAll()
        detachToolbar()
        nativeToolbar.delegate = nil
        (splitView as? TrackerWindowSplitView)?.onWindowChange = nil
        openSettings = {}
    }

    private func actionItem(_ id: NSToolbarItem.Identifier, label: String, symbol: String,
                            action: Selector) -> NSToolbarItem {
        let item = NSToolbarItem(itemIdentifier: id)
        item.label = label
        item.paletteLabel = label
        item.toolTip = label
        item.image = NSImage(systemSymbolName: symbol, accessibilityDescription: label)
        item.target = self
        item.action = action
        item.autovalidates = false
        return item
    }

    private func group(_ id: NSToolbarItem.Identifier, label: String, items: [NSToolbarItem],
                       priority: NSToolbarItem.VisibilityPriority) -> NSToolbarItemGroup {
        let group = NSToolbarItemGroup(itemIdentifier: id)
        group.label = label
        group.paletteLabel = label
        group.selectionMode = .momentary
        group.subitems = items
        group.autovalidates = false
        group.visibilityPriority = priority
        return group
    }

    private func updateToolbarContent() {
        guard !isTornDown, installedWindow != nil else { return }
        let task = store.selectedTask
        let isRunning = task != nil && store.active?.taskId == task?.id
        let canTrack: Bool
        if let task, !task.archived {
            canTrack = isRunning ? store.activity.canStopTracking
                : store.activity.canStartTracking(taskID: task.id)
        } else {
            canTrack = false
        }
        let content = ToolbarContent(
            title: task?.name ?? "Time Tracker", hasSelectedTask: task != nil,
            isArchived: task?.archived ?? false, canCreate: canOpenCreation,
            canRename: task != nil && canOpenRename, isRunning: isRunning,
            canTrack: canTrack, hasActiveTimer: store.active != nil
        )
        guard renderedContent != content else { return }
        renderedContent = content
        installedWindow?.title = content.title
        creationItem.isEnabled = content.canCreate
        renameItem.isEnabled = content.canRename
        let detailIndex = nativeToolbar.items.firstIndex { $0.itemIdentifier == ItemID.detail }
        if let task {
            let items = task.archived ? [renameItem] : [renameItem, trackingItem]
            if detailControls.subitems.map(\.itemIdentifier) != items.map(\.itemIdentifier) {
                detailControls.subitems = items
            }
            if detailIndex == nil {
                nativeToolbar.insertItem(withItemIdentifier: ItemID.detail, at: nativeToolbar.items.count)
            }
        } else if let detailIndex {
            nativeToolbar.removeItem(at: detailIndex)
        }
        let trackingLabel = isRunning ? "Stop tracking" : "Start tracking"
        if trackingItem.label != trackingLabel {
            trackingItem.label = trackingLabel
            trackingItem.image = NSImage(systemSymbolName: isRunning ? "stop.fill" : "play.fill",
                                         accessibilityDescription: trackingLabel)
        }
        trackingItem.toolTip = isRunning ? "Stop tracking this task"
            : store.active == nil ? "Start tracking this task"
            : "Stop the current timer and start tracking this task"
        trackingItem.isEnabled = content.canTrack
    }

    private func updateSidebarSection() {
        guard !isTornDown, installedWindow != nil, !updatingSeparator else { return }
        updatingSeparator = true
        defer { updatingSeparator = false }
        let collapsed = sidebarItem.isCollapsed
        let label = collapsed ? "Show sidebar" : "Hide sidebar"
        if collapseItem.label != label {
            collapseItem.label = label
            collapseItem.toolTip = label
            collapseItem.image?.accessibilityDescription = label
        }
        if collapsed {
            if let index = nativeToolbar.items.firstIndex(where: { $0.itemIdentifier == ItemID.separator }) {
                nativeToolbar.removeItem(at: index)
            }
            if let groupIndex = nativeToolbar.items.firstIndex(where: { $0.itemIdentifier == ItemID.sidebar }),
               groupIndex > 0, nativeToolbar.items[groupIndex - 1].itemIdentifier == .flexibleSpace {
                // Keep the restore button at the leading edge when there is no sidebar section.
                nativeToolbar.removeItem(at: groupIndex - 1)
            }
        } else {
            if let groupIndex = nativeToolbar.items.firstIndex(where: { $0.itemIdentifier == ItemID.sidebar }),
               groupIndex == 0 || nativeToolbar.items[groupIndex - 1].itemIdentifier != .flexibleSpace {
                nativeToolbar.insertItem(withItemIdentifier: .flexibleSpace, at: groupIndex)
            }
            if !nativeToolbar.items.contains(where: { $0.itemIdentifier == ItemID.separator }),
               let groupIndex = nativeToolbar.items.firstIndex(where: { $0.itemIdentifier == ItemID.sidebar }) {
                nativeToolbar.insertItem(withItemIdentifier: ItemID.separator, at: groupIndex + 1)
            }
        }
    }

    @objc private func toggleSidebarAction() {
        toggleSidebar(nil)
        updateSidebarSection()
    }

    @objc private func showSettings() { openSettings() }

    private var hasTaskEditor: Bool {
        store.creation.state.isPresented || store.rename.state.isPresented
    }

    private var canOpenCreation: Bool { store.creation.canOpen && !hasTaskEditor }
    private var canOpenRename: Bool { store.rename.canOpen && !hasTaskEditor }

    @objc private func createTask() {
        guard canOpenCreation else { return }
        store.creation.open()
    }

    @objc private func renameTask() {
        guard canOpenRename, let task = store.selectedTask else { return }
        store.rename.open(taskID: task.id)
    }

    @objc private func changeTracking() {
        guard let task = store.selectedTask, !task.archived else { return }
        if let active = store.active, active.taskId == task.id {
            guard store.activity.canStopTracking else { return }
            store.stopTracking(worklogID: active.id)
        } else {
            guard store.activity.canStartTracking(taskID: task.id) else { return }
            store.startTracking(taskID: task.id)
        }
    }

    func toolbarDefaultItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        [.flexibleSpace, ItemID.sidebar, ItemID.separator, .flexibleSpace, ItemID.detail]
    }

    func toolbarAllowedItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        toolbarDefaultItemIdentifiers(toolbar)
    }

    func toolbar(_ toolbar: NSToolbar, itemForItemIdentifier itemIdentifier: NSToolbarItem.Identifier,
                 willBeInsertedIntoToolbar flag: Bool) -> NSToolbarItem? {
        switch itemIdentifier {
        case ItemID.sidebar: return sidebarControls
        case ItemID.separator:
            return NSTrackingSeparatorToolbarItem(identifier: ItemID.separator, splitView: splitView, dividerIndex: 0)
        case ItemID.detail: return detailControls
        default: return nil
        }
    }
}

@MainActor
private final class TrackerWindowSplitView: NSSplitView {
    var onWindowChange: ((NSWindow?) -> Void)?

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        onWindowChange?(window)
    }
}
