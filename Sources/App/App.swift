import SwiftUI

@main
struct QuickSailorApp: App {
    private let repository = InMemoryTaskRepository(tasks: [
        makeTask(title: "Write project plan", description: "Capture the current decisions and next checkpoints."),
        makeTask(title: "Review task list"),
    ])

    var body: some Scene {
        WindowGroup {
            ContentView(viewModel: TaskListViewModel(repository: repository))
        }
    }
}

private func makeTask(title: String, description: String? = nil) -> Task {
    do {
        return try Task(title: title, description: description)
    } catch {
        fatalError("Failed to create seeded task: \(error)")
    }
}
