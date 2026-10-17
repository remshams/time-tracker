import Foundation

/// Runs feature workflows behind the session's single operation gate.
///
/// `run` passes `work` the session generation that started it. A shutdown
/// advances the generation, so work must check `isCurrent` after each await.
@MainActor
protocol OperationRunner: AnyObject {
    /// The session is running and no operation holds the gate.
    var isIdle: Bool { get }
    var isSleeping: Bool { get }
    func isCurrent(_ token: Int) -> Bool
    /// Takes the gate, runs `work`, and releases the gate when `work` returns.
    func run(blocksControls: Bool, _ work: @escaping @MainActor (_ token: Int) async -> Void)
    func publish()
}
