import Foundation

struct Task: Identifiable, Equatable {
    enum ValidationError: Error, Equatable {
        case emptyTitle
    }

    let id: UUID
    let title: String
    let description: String?

    init(id: UUID = UUID(), title: String, description: String? = nil) throws {
        guard title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == false else {
            throw ValidationError.emptyTitle
        }

        self.id = id
        self.title = title
        self.description = description
    }
}
