import Foundation

struct WorkTask: Identifiable, Equatable, Sendable {
  enum ValidationError: Error, Equatable {
    case emptyTitle
  }

  nonisolated let id: UUID
  let title: String
  let description: String?

  nonisolated init(id: UUID = UUID(), title: String, description: String? = nil) throws {
    guard !title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
      throw ValidationError.emptyTitle
    }

    self.id = id
    self.title = title
    self.description = description
  }

  nonisolated static func == (lhs: WorkTask, rhs: WorkTask) -> Bool {
    lhs.id == rhs.id && lhs.title == rhs.title && lhs.description == rhs.description
  }
}
