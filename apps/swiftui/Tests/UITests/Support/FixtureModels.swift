import Foundation

struct FixtureTask: Decodable {
    let id: String
    let name: String
    let archived: Bool
}

struct FixtureWorklog: Decodable, Equatable {
    let id: String
    let taskID: String
    let start: String
    let end: String?

    enum CodingKeys: String, CodingKey {
        case id, start, end
        case taskID = "task_id"
    }
}

struct FixtureReport: Decodable {
    let totalMicroseconds: Int64
    let rows: [Row]
    struct Row: Decodable {
        let task: FixtureTask
        let durationMicroseconds: Int64
        enum CodingKeys: String, CodingKey {
            case task
            case durationMicroseconds = "duration_us"
        }
    }
    enum CodingKeys: String, CodingKey {
        case rows
        case totalMicroseconds = "total_us"
    }
}

struct FixtureError: LocalizedError {
    let errorDescription: String?
    init(_ description: String) { errorDescription = description }
}
