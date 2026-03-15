import Testing

@testable import App

@MainActor
@Test func contentViewModuleLoads() {
    let repository = InMemoryTaskRepository(tasks: [
        try! Task(title: "Write project plan", description: "Capture the current decisions."),
        try! Task(title: "Review next step"),
    ])
    let view = ContentView(viewModel: TaskListViewModel(repository: repository))

    #expect(String(describing: type(of: view)) == "ContentView")
}

@MainActor
@Test func taskListViewModuleLoads() {
    let repository = InMemoryTaskRepository(tasks: [
        try! Task(title: "Write project plan", description: "Capture the current decisions."),
        try! Task(title: "Review next step"),
    ])
    let view = TaskListView(viewModel: TaskListViewModel(repository: repository))

    #expect(String(describing: type(of: view)) == "TaskListView")
}
