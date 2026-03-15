import Foundation

struct Task: Identifiable, Equatable {
    let id: UUID
    let title: String
    let description: String?

    init(id: UUID = UUID(), title: String, description: String? = nil) {
        self.id = id
        self.title = title
        self.description = description
    }
}
