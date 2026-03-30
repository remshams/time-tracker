import Testing

@testable import TimeTracker

@Suite struct WorkTaskListViewModelTests {
  @Suite @MainActor struct LoadTasks {
    @Test func loadsTasksFromRepository() async {
      let tasks = [
        TestFactories.makeTask(description: "Capture the current decisions."),
        TestFactories.makeTask(),
      ]
      let viewModel = WorkTaskListViewModel(repository: WorkTaskRepositoryStub(result: .success(tasks)))

      await viewModel.loadTasks()

      #expect(viewModel.tasks == tasks)
      #expect(viewModel.errorMessage == nil)
      #expect(viewModel.isLoading == false)
    }

    @Test func setsErrorMessageWhenLoadingFails() async {
      let viewModel = WorkTaskListViewModel(
        repository: WorkTaskRepositoryStub(result: .failure(WorkTaskRepositoryStubError.fetchFailed)))

      await viewModel.loadTasks()

      #expect(viewModel.tasks.isEmpty)
      #expect(viewModel.errorMessage == "Failed to load tasks.")
      #expect(viewModel.isLoading == false)
    }

    @Test func isNotLoadedBeforeFirstLoad() {
      let viewModel = WorkTaskListViewModel(
        repository: WorkTaskRepositoryStub(result: .success([])))

      #expect(viewModel.isLoaded == false)
    }

    @Test func isLoadedAfterSuccessfulLoad() async {
      let viewModel = WorkTaskListViewModel(
        repository: WorkTaskRepositoryStub(result: .success([])))

      await viewModel.loadTasks()

      #expect(viewModel.isLoaded == true)
    }

    @Test func isNotLoadedAfterFailedLoad() async {
      let viewModel = WorkTaskListViewModel(
        repository: WorkTaskRepositoryStub(result: .failure(WorkTaskRepositoryStubError.fetchFailed)))

      await viewModel.loadTasks()

      #expect(viewModel.isLoaded == false)
    }
  }

  @Suite @MainActor struct TaskLookup {
    @Test func returnsTaskForKnownID() async {
      let task = TestFactories.makeTask()
      let viewModel = WorkTaskListViewModel(repository: WorkTaskRepositoryStub(result: .success([task])))

      await viewModel.loadTasks()

      #expect(viewModel.task(for: task.id) == task)
    }

    @Test func returnsNilForUnknownID() async {
      let task = TestFactories.makeTask()
      let unknownID = TestFactories.makeTask().id
      let viewModel = WorkTaskListViewModel(repository: WorkTaskRepositoryStub(result: .success([task])))

      await viewModel.loadTasks()

      #expect(viewModel.task(for: unknownID) == nil)
    }
  }

  @Suite @MainActor struct CreateTask {
    @Test func addsTaskToRepository() async {
      let newTask = TestFactories.makeTask(title: "New task")
      let stub = WorkTaskRepositoryStub(result: .success([]))
      stub.resultAfterAdd = .success([newTask])
      let viewModel = WorkTaskListViewModel(repository: stub)

      await viewModel.createTask(title: newTask.title, description: newTask.description ?? "")

      #expect(viewModel.tasks == [newTask])
    }

    @Test func reloadsTasksOnSuccess() async {
      let newTask = TestFactories.makeTask(title: "New task")
      let stub = WorkTaskRepositoryStub(result: .success([]))
      stub.resultAfterAdd = .success([newTask])
      let viewModel = WorkTaskListViewModel(repository: stub)

      await viewModel.createTask(title: newTask.title, description: "")

      #expect(viewModel.tasks == [newTask])
      #expect(viewModel.isLoaded == true)
    }

    @Test func setsErrorMessageWhenRepositoryThrows() async {
      let stub = WorkTaskRepositoryStub(result: .success([]))
      stub.addTaskResult = .failure(WorkTaskRepositoryStubError.addFailed)
      let viewModel = WorkTaskListViewModel(repository: stub)

      await viewModel.createTask(title: "New task", description: "")

      #expect(viewModel.errorMessage == "Failed to create task.")
    }

    @Test func doesNotUpdateTasksWhenRepositoryThrows() async {
      let existingTask = TestFactories.makeTask(title: "Existing task")
      let stub = WorkTaskRepositoryStub(result: .success([existingTask]))
      stub.addTaskResult = .failure(WorkTaskRepositoryStubError.addFailed)
      let viewModel = WorkTaskListViewModel(repository: stub)
      await viewModel.loadTasks()

      await viewModel.createTask(title: "New task", description: "")

      #expect(viewModel.tasks == [existingTask])
    }

    @Test func setsErrorWhenTitleIsWhitespaceOnly() async {
      let stub = WorkTaskRepositoryStub(result: .success([]))
      let viewModel = WorkTaskListViewModel(repository: stub)

      await viewModel.createTask(title: "   ", description: "")

      #expect(viewModel.errorMessage == "Failed to create task.")
      #expect(viewModel.tasks.isEmpty)
    }

    @Test func clearsErrorStateAfterSuccessfulCreate() async {
      let newTask = TestFactories.makeTask(title: "New task")
      let stub = WorkTaskRepositoryStub(result: .failure(WorkTaskRepositoryStubError.fetchFailed))
      let viewModel = WorkTaskListViewModel(repository: stub)
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

private final class WorkTaskRepositoryStub: WorkTaskRepository, @unchecked Sendable {
  var result: Result<[WorkTask], Error>
  var resultAfterAdd: Result<[WorkTask], Error>?
  var addTaskResult: Result<Void, Error> = .success(())

  private var addTaskCalled = false

  init(result: Result<[WorkTask], Error>) {
    self.result = result
  }

  func fetchTasks() async throws -> [WorkTask] {
    if addTaskCalled, let resultAfterAdd {
      return try resultAfterAdd.get()
    }
    return try result.get()
  }

  func addTask(_ task: WorkTask) async throws {
    try addTaskResult.get()
    addTaskCalled = true
  }
}

private enum WorkTaskRepositoryStubError: Error, Sendable {
  case fetchFailed
  case addFailed
}
