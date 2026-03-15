import SwiftUI

@main
struct QuickSailorApp: App {
    private let repository = InMemoryTaskRepository(tasks: [
        try! Task(title: "Write project plan", description: "Capture the current decisions and next checkpoints."),
        try! Task(title: "Review task list"),
    ])

    var body: some Scene {
        WindowGroup {
            ContentView(viewModel: TaskListViewModel(repository: repository))
        }
    }
}
