import Testing

@testable import App

@MainActor
@Test func taskListViewModelLoadsTasksFromRepository() async {
  let tasks = [
    TestFactories.makeTask(title: TestFactories.anyTaskTitle, description: "Capture the current decisions."),
    TestFactories.makeTask(title: TestFactories.anyTaskTitle),
  ]
  let viewModel = TaskListViewModel(repository: TaskRepositoryStub(result: .success(tasks)))

  await viewModel.loadTasks()

  #expect(viewModel.tasks == tasks)
  #expect(viewModel.errorMessage == nil)
  #expect(viewModel.isLoading == false)
}

@MainActor
@Test func taskListViewModelStoresAnErrorMessageWhenLoadingFails() async {
  let viewModel = TaskListViewModel(
    repository: TaskRepositoryStub(result: .failure(TaskRepositoryStubError.fetchFailed)))

  await viewModel.loadTasks()

  #expect(viewModel.tasks.isEmpty)
  #expect(viewModel.errorMessage == "Failed to load tasks.")
  #expect(viewModel.isLoading == false)
}

@MainActor
@Test func taskListViewModelIsNotLoadedBeforeFirstLoad() {
  let viewModel = TaskListViewModel(
    repository: TaskRepositoryStub(result: .success([])))

  #expect(viewModel.isLoaded == false)
}

@MainActor
@Test func taskListViewModelIsLoadedAfterSuccessfulLoad() async {
  let viewModel = TaskListViewModel(
    repository: TaskRepositoryStub(result: .success([])))

  await viewModel.loadTasks()

  #expect(viewModel.isLoaded == true)
}

@MainActor
@Test func taskListViewModelIsNotLoadedAfterFailedLoad() async {
  let viewModel = TaskListViewModel(
    repository: TaskRepositoryStub(result: .failure(TaskRepositoryStubError.fetchFailed)))

  await viewModel.loadTasks()

  #expect(viewModel.isLoaded == false)
}

@MainActor
@Test func taskListViewModelReturnsTaskForKnownID() async {
  let task = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
  let viewModel = TaskListViewModel(repository: TaskRepositoryStub(result: .success([task])))

  await viewModel.loadTasks()

  #expect(viewModel.task(for: task.id) == task)
}

@MainActor
@Test func taskListViewModelReturnsNilForUnknownID() async {
  let task = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
  let unknownID = TestFactories.makeTask(title: TestFactories.anyTaskTitle).id
  let viewModel = TaskListViewModel(repository: TaskRepositoryStub(result: .success([task])))

  await viewModel.loadTasks()

  #expect(viewModel.task(for: unknownID) == nil)
}

private final class TaskRepositoryStub: TaskRepository, @unchecked Sendable {
  var result: Result<[Task], Error>

  init(result: Result<[Task], Error>) {
    self.result = result
  }

  func fetchTasks() async throws -> [Task] {
    try result.get()
  }

  func addTask(_ task: Task) async throws {}
}

private enum TaskRepositoryStubError: Error, Sendable {
  case fetchFailed
}
