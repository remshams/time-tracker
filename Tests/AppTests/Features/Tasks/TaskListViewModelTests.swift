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

@MainActor
@Test func taskListViewModelCreateTaskAddsTaskToRepository() async {
  let newTask = TestFactories.makeTask(title: "New task")
  let stub = TaskRepositoryStub(result: .success([]))
  stub.resultAfterAdd = .success([newTask])
  let viewModel = TaskListViewModel(repository: stub)

  await viewModel.createTask(title: newTask.title, description: newTask.description ?? "")

  #expect(viewModel.tasks == [newTask])
}

@MainActor
@Test func taskListViewModelCreateTaskReloadsTasksOnSuccess() async {
  let newTask = TestFactories.makeTask(title: "New task")
  let stub = TaskRepositoryStub(result: .success([]))
  stub.resultAfterAdd = .success([newTask])
  let viewModel = TaskListViewModel(repository: stub)

  await viewModel.createTask(title: newTask.title, description: "")

  #expect(viewModel.tasks == [newTask])
  #expect(viewModel.isLoaded == true)
}

@MainActor
@Test func taskListViewModelCreateTaskSetsErrorMessageWhenRepositoryThrows() async {
  let stub = TaskRepositoryStub(result: .success([]))
  stub.addTaskResult = .failure(TaskRepositoryStubError.addFailed)
  let viewModel = TaskListViewModel(repository: stub)

  await viewModel.createTask(title: "New task", description: "")

  #expect(viewModel.errorMessage == "Failed to create task.")
}

@MainActor
@Test func taskListViewModelCreateTaskDoesNotUpdateTasksWhenRepositoryThrows() async {
  let existingTask = TestFactories.makeTask(title: "Existing task")
  let stub = TaskRepositoryStub(result: .success([existingTask]))
  stub.addTaskResult = .failure(TaskRepositoryStubError.addFailed)
  let viewModel = TaskListViewModel(repository: stub)
  await viewModel.loadTasks()

  await viewModel.createTask(title: "New task", description: "")

  #expect(viewModel.tasks == [existingTask])
}

private final class TaskRepositoryStub: TaskRepository, @unchecked Sendable {
  let result: Result<[Task], Error>
  var resultAfterAdd: Result<[Task], Error>?
  var addTaskResult: Result<Void, Error> = .success(())

  private var fetchCallCount = 0

  init(result: Result<[Task], Error>) {
    self.result = result
  }

  func fetchTasks() async throws -> [Task] {
    defer { fetchCallCount += 1 }
    if fetchCallCount > 0, let resultAfterAdd {
      return try resultAfterAdd.get()
    }
    return try result.get()
  }

  func addTask(_ task: Task) async throws {
    try addTaskResult.get()
  }
}

private enum TaskRepositoryStubError: Error, Sendable {
  case fetchFailed
  case addFailed
}
