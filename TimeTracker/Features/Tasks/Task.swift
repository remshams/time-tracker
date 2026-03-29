import Foundation

struct Task: Identifiable, Equatable, Sendable {
  enum ValidationError: Error, Equatable {
    case emptyTitle
  }

  nonisolated let id: UUID
  nonisolated let title: String
  nonisolated let description: String?

  nonisolated init(id: UUID = UUID(), title: String, description: String? = nil) throws {
    guard !title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
      throw ValidationError.emptyTitle
    }

    self.id = id
    self.title = title
    self.description = description
  }

  nonisolated static func == (lhs: Task, rhs: Task) -> Bool {
    lhs.id == rhs.id && lhs.title == rhs.title && lhs.description == rhs.description
  }
}
