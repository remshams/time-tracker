import Foundation

@MainActor
public final class DelayedProgressPresentation {
    public static let delay: Duration = .milliseconds(180)
    public typealias Sleep = @Sendable (Duration) async throws -> Void
    private let sleep: Sleep
    private var task: Task<Void, Never>?
    private var isBusy = false

    public init(sleep: @escaping Sleep = { try await Task.sleep(for: $0) }) {
        self.sleep = sleep
    }

    public func update(isBusy: Bool, visibility: @escaping @MainActor (Bool) -> Void) {
        guard self.isBusy != isBusy else { return }
        self.isBusy = isBusy
        task?.cancel()
        task = nil
        visibility(false)
        guard isBusy else { return }
        let sleep = self.sleep
        let delay = Self.delay
        // Every change that makes this reveal stale cancels the task on the main actor first.
        task = Task {
            do { try await sleep(delay) } catch { return }
            guard !Task.isCancelled else { return }
            visibility(true)
        }
    }

    public func cancel() {
        isBusy = false
        task?.cancel()
        task = nil
    }

    deinit { task?.cancel() }
}
