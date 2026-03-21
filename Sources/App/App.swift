import SwiftUI

@main
struct TimeTracker: App {
    private let taskRepository: InMemoryTaskRepository
    private let workLogRepository: InMemoryWorkLogRepository

    init() {
        let planTask = makeTask(
            title: "Write project plan",
            description: "Capture the current decisions and next checkpoints.")
        let reviewTask = makeTask(title: "Review task list")

        taskRepository = InMemoryTaskRepository(tasks: [planTask, reviewTask])

        workLogRepository = InMemoryWorkLogRepository(entriesByTaskID: [
            planTask.id: [
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
            ],
            reviewTask.id: [
                makeWorkLogEntry(
                    taskID: reviewTask.id,
                    description: "Review assumptions",
                    startedAt: makeDate(hour: 10, minute: 0),
                    endedAt: makeDate(hour: 10, minute: 20))
            ],
        ])
    }

    var body: some Scene {
        WindowGroup {
            ContentView(
                taskListViewModel: TaskListViewModel(repository: taskRepository),
                workLogListViewModel: WorkLogListViewModel(repository: workLogRepository))
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

private func makeWorkLogEntry(
    taskID: Task.ID,
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
    return Calendar.current.date(from: components) ?? .now
}
