import Testing

@testable import App

@MainActor
@Test func contentViewModuleLoads() {
    let repository = InMemoryTaskRepository(tasks: [
        TestFactories.makeTask(title: "Write project plan", description: "Capture the current decisions."),
        TestFactories.makeTask(title: "Review next step"),
    ])

    let view = ContentView(viewModel: TaskListViewModel(repository: repository))

    #expect(type(of: view) == ContentView.self)
}

@MainActor
@Test func taskListViewModuleLoads() {
    let repository = InMemoryTaskRepository(tasks: [
        TestFactories.makeTask(title: "Write project plan", description: "Capture the current decisions."),
        TestFactories.makeTask(title: "Review next step"),
    ])

    let view = TaskListView(viewModel: TaskListViewModel(repository: repository))

    #expect(type(of: view) == TaskListView.self)
}
