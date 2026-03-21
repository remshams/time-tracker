import Foundation

struct Task: Identifiable, Equatable, Sendable {
    enum ValidationError: Error, Equatable {
        case emptyTitle
    }

    let id: UUID
    let title: String
    let description: String?

    init(id: UUID = UUID(), title: String, description: String? = nil) throws {
        guard !title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            throw ValidationError.emptyTitle
        }

        self.id = id
        self.title = title
        self.description = description
    }
}
