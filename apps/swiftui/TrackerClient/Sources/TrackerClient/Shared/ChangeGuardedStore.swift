/// An observable store that assigns a property only when its value changes.
///
/// Observation notifies a property's readers on every assignment, including an equal value. Stores write
/// through `set`, which compares with `==` first, so an unchanged poll or tick redraws nothing.
protocol ChangeGuardedStore: AnyObject {}

extension ChangeGuardedStore {
    func set<Value: Equatable>(_ keyPath: ReferenceWritableKeyPath<Self, Value>, _ value: Value) {
        guard self[keyPath: keyPath] != value else { return }
        self[keyPath: keyPath] = value
    }
}
