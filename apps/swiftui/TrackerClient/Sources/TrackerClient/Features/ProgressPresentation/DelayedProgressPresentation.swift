import Foundation

@MainActor
public final class DelayedProgressPresentation {
    public static let delay: Duration = .milliseconds(180)
    public typealias Sleep = @Sendable (Duration) async throws -> Void
    private let sleep: Sleep
    private var task: Task<Void, Never>?
    private var generation = 0
    private var isBusy = false

    public init(sleep: @escaping Sleep = { try await Task.sleep(for: $0) }) {
        self.sleep = sleep
    }

    public func update(isBusy: Bool, visibility: @escaping @MainActor (Bool) -> Void) {
        guard self.isBusy != isBusy else { return }
        self.isBusy = isBusy
        generation += 1
        task?.cancel()
        task = nil
        visibility(false)
        guard isBusy else { return }
        let request = generation
        let sleep = self.sleep
        task = Task { [weak self] in
            do { try await sleep(Self.delay) } catch { return }
            guard !Task.isCancelled, let self, self.generation == request, self.isBusy else { return }
            visibility(true)
        }
    }

    public func cancel() {
        generation += 1
        isBusy = false
        task?.cancel()
        task = nil
    }

    deinit { task?.cancel() }
}
