import Testing

@testable import App

@Suite @MainActor struct TaskListViewModelTests {
  @Suite @MainActor struct LoadTasks {
    @Test func loadsTasksFromRepository() async {
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

    @Test func setsErrorMessageWhenLoadingFails() async {
      let viewModel = TaskListViewModel(
        repository: TaskRepositoryStub(result: .failure(TaskRepositoryStubError.fetchFailed)))

      await viewModel.loadTasks()

      #expect(viewModel.tasks.isEmpty)
      #expect(viewModel.errorMessage == "Failed to load tasks.")
      #expect(viewModel.isLoading == false)
    }

    @Test func isNotLoadedBeforeFirstLoad() {
      let viewModel = TaskListViewModel(
        repository: TaskRepositoryStub(result: .success([])))

      #expect(viewModel.isLoaded == false)
    }

    @Test func isLoadedAfterSuccessfulLoad() async {
      let viewModel = TaskListViewModel(
        repository: TaskRepositoryStub(result: .success([])))

      await viewModel.loadTasks()

      #expect(viewModel.isLoaded == true)
    }

    @Test func isNotLoadedAfterFailedLoad() async {
      let viewModel = TaskListViewModel(
        repository: TaskRepositoryStub(result: .failure(TaskRepositoryStubError.fetchFailed)))

      await viewModel.loadTasks()

      #expect(viewModel.isLoaded == false)
    }
  }

  @Suite @MainActor struct TaskLookup {
    @Test func returnsTaskForKnownID() async {
      let task = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
      let viewModel = TaskListViewModel(repository: TaskRepositoryStub(result: .success([task])))

      await viewModel.loadTasks()

      #expect(viewModel.task(for: task.id) == task)
    }

    @Test func returnsNilForUnknownID() async {
      let task = TestFactories.makeTask(title: TestFactories.anyTaskTitle)
      let unknownID = TestFactories.makeTask(title: TestFactories.anyTaskTitle).id
      let viewModel = TaskListViewModel(repository: TaskRepositoryStub(result: .success([task])))

      await viewModel.loadTasks()

      #expect(viewModel.task(for: unknownID) == nil)
    }
  }

  @Suite @MainActor struct CreateTask {
    @Test func addsTaskToRepository() async {
      let newTask = TestFactories.makeTask(title: "New task")
      let stub = TaskRepositoryStub(result: .success([]))
      stub.resultAfterAdd = .success([newTask])
      let viewModel = TaskListViewModel(repository: stub)

      await viewModel.createTask(title: newTask.title, description: newTask.description ?? "")

      #expect(viewModel.tasks == [newTask])
    }

    @Test func reloadsTasksOnSuccess() async {
      let newTask = TestFactories.makeTask(title: "New task")
      let stub = TaskRepositoryStub(result: .success([]))
      stub.resultAfterAdd = .success([newTask])
      let viewModel = TaskListViewModel(repository: stub)

      await viewModel.createTask(title: newTask.title, description: "")

      #expect(viewModel.tasks == [newTask])
      #expect(viewModel.isLoaded == true)
    }

    @Test func setsErrorMessageWhenRepositoryThrows() async {
      let stub = TaskRepositoryStub(result: .success([]))
      stub.addTaskResult = .failure(TaskRepositoryStubError.addFailed)
      let viewModel = TaskListViewModel(repository: stub)

      await viewModel.createTask(title: "New task", description: "")

      #expect(viewModel.errorMessage == "Failed to create task.")
    }

    @Test func doesNotUpdateTasksWhenRepositoryThrows() async {
      let existingTask = TestFactories.makeTask(title: "Existing task")
      let stub = TaskRepositoryStub(result: .success([existingTask]))
      stub.addTaskResult = .failure(TaskRepositoryStubError.addFailed)
      let viewModel = TaskListViewModel(repository: stub)
      await viewModel.loadTasks()

      await viewModel.createTask(title: "New task", description: "")

      #expect(viewModel.tasks == [existingTask])
    }

    @Test func setsErrorWhenTitleIsWhitespaceOnly() async {
      let stub = TaskRepositoryStub(result: .success([]))
      let viewModel = TaskListViewModel(repository: stub)

      await viewModel.createTask(title: "   ", description: "")

      #expect(viewModel.errorMessage == "Failed to create task.")
      #expect(viewModel.tasks.isEmpty)
    }

    @Test func clearsErrorStateAfterSuccessfulCreate() async {
      let newTask = TestFactories.makeTask(title: "New task")
      let stub = TaskRepositoryStub(result: .failure(TaskRepositoryStubError.fetchFailed))
      let viewModel = TaskListViewModel(repository: stub)
      await viewModel.loadTasks()
      #expect(viewModel.errorMessage != nil)

      stub.addTaskResult = .success(())
      stub.resultAfterAdd = .success([newTask])
      await viewModel.createTask(title: newTask.title, description: "")

      #expect(viewModel.errorMessage == nil)
      #expect(viewModel.isLoaded == true)
    }
  }
}

// MARK: - Test doubles

private final class TaskRepositoryStub: TaskRepository, @unchecked Sendable {
  var result: Result<[Task], Error>
  var resultAfterAdd: Result<[Task], Error>?
  var addTaskResult: Result<Void, Error> = .success(())

  private var addTaskCalled = false

  init(result: Result<[Task], Error>) {
    self.result = result
  }

  func fetchTasks() async throws -> [Task] {
    if addTaskCalled, let resultAfterAdd {
      return try resultAfterAdd.get()
    }
    return try result.get()
  }

  func addTask(_ task: Task) async throws {
    try addTaskResult.get()
    addTaskCalled = true
  }
}

private enum TaskRepositoryStubError: Error, Sendable {
  case fetchFailed
  case addFailed
}
