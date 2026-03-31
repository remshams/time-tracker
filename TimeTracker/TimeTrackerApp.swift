import SwiftUI

@main
struct TimeTracker: App {
  private let workTaskRepository: any WorkTaskRepository
  private let workLogRepository: any WorkLogRepository
  private let trackingService: WorkLogTrackingService

  init() {
    workTaskRepository = InMemoryWorkTaskRepository()
    workLogRepository = InMemoryWorkLogRepository()
    trackingService = WorkLogTrackingService()
  }

  var body: some Scene {
    WindowGroup {
      ContentView(
        workTaskListViewModel: WorkTaskListViewModel(repository: workTaskRepository),
        workLogListViewModel: WorkLogListViewModel(
          repository: workLogRepository,
          trackingService: trackingService)
      )
      .environment(trackingService)
      .task {
        if let running = try? await workLogRepository.fetchRunningEntry() {
          trackingService.start(running)
        }
      }
    }
  }
}
