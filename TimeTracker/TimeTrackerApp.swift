import SwiftUI

@main
struct TimeTracker: App {
  private let workTaskRepository: any WorkTaskRepository
  private let workLogRepository: any WorkLogRepository
  private let trackingService: WorkLogTrackingService

  init() {
    let planTask = makeTask(
      title: "Write project plan",
      description: "Capture the current decisions and next checkpoints.")
    let reviewTask = makeTask(title: "Review task list")

    workTaskRepository = InMemoryWorkTaskRepository(tasks: [planTask, reviewTask])

    let planEntries: [WorkLogEntry] = [
      makeWorkLogEntry(
        taskID: planTask.id,
        description: "Initial architecture and constraints",
        startedAt: makeDate(hour: 9, minute: 0),
        endedAt: makeDate(hour: 10, minute: 30)),
      makeWorkLogEntry(
        taskID: planTask.id,
        description: "Draft implementation steps",
        startedAt: makeDate(hour: 11, minute: 0),
        endedAt: makeDate(hour: 11, minute: 45)),
      makeWorkLogEntry(
        taskID: planTask.id,
        startedAt: makeDate(hour: 14, minute: 0)),
    ]
    let reviewEntries: [WorkLogEntry] = [
      makeWorkLogEntry(
        taskID: reviewTask.id,
        description: "Review assumptions",
        startedAt: makeDate(hour: 10, minute: 0),
        endedAt: makeDate(hour: 10, minute: 20)),
      makeWorkLogEntry(
        taskID: reviewTask.id,
        description: "Align on next steps",
        startedAt: makeDate(hour: 14, minute: 30),
        endedAt: makeDate(hour: 14, minute: 50)),
    ]
    workLogRepository = InMemoryWorkLogRepository(entriesByTaskID: [
      planTask.id: planEntries,
      reviewTask.id: reviewEntries,
    ])
    trackingService = WorkLogTrackingService()
    let service = trackingService
    let logRepository = workLogRepository
    Task {
      if let running = try? await logRepository.fetchRunningEntry() {
        service.start(running)
      }
    }
  }

  var body: some Scene {
    WindowGroup {
      ContentView(
        workTaskListViewModel: WorkTaskListViewModel(repository: workTaskRepository),
        workLogListViewModel: WorkLogListViewModel(repository: workLogRepository)
      )
      .environment(trackingService)
    }
  }
}

private func makeTask(title: String, description: String? = nil) -> WorkTask {
  do {
    return try WorkTask(title: title, description: description)
  } catch {
    fatalError("Failed to create seeded task: \(error)")
  }
}

private func makeWorkLogEntry(
  taskID: WorkTask.ID,
  description: String? = nil,
  startedAt: Date,
  endedAt: Date? = nil
) -> WorkLogEntry {
  do {
    return try WorkLogEntry(
      taskID: taskID,
      description: description,
      startedAt: startedAt,
      endedAt: endedAt)
  } catch {
    fatalError("Failed to create seeded work log entry: \(error)")
  }
}

private func makeDate(hour: Int, minute: Int) -> Date {
  var components = Calendar.current.dateComponents([.year, .month, .day], from: .now)
  components.hour = hour
  components.minute = minute
  components.second = 0
  guard let date = Calendar.current.date(from: components) else {
    fatalError("Failed to construct seed date for \(hour):\(minute)")
  }
  return date
}
