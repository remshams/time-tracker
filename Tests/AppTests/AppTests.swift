import Testing

@testable import App

@Test func contentViewModuleLoads() async {
    let view = await MainActor.run {
        let repository = InMemoryTaskRepository(tasks: [
            makeTask(title: "Write project plan", description: "Capture the current decisions."),
            makeTask(title: "Review next step"),
        ])

        return ContentView(viewModel: TaskListViewModel(repository: repository))
    }

    #expect(String(describing: type(of: view)) == "ContentView")
}

@Test func taskListViewModuleLoads() async {
    let view = await MainActor.run {
        let repository = InMemoryTaskRepository(tasks: [
            makeTask(title: "Write project plan", description: "Capture the current decisions."),
            makeTask(title: "Review next step"),
        ])

        return TaskListView(viewModel: TaskListViewModel(repository: repository))
    }

    #expect(String(describing: type(of: view)) == "TaskListView")
}

private func makeTask(title: String, description: String? = nil) -> Task {
    do {
        return try Task(title: title, description: description)
    } catch {
        Issue.record("Failed to create test task: \(error)")
        fatalError("Failed to create test task: \(error)")
    }
}
