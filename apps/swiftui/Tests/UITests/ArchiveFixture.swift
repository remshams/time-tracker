import Foundation

struct ArchiveSeed {
    let old: FixtureTask
    let medium: FixtureTask
    let recent: FixtureTask
    let running: FixtureTask
    let worklog: FixtureWorklog
}

typealias ArchiveFixture = TrackerFixture

extension TrackerFixture {
    func seed() throws -> ArchiveSeed {
        let now = Date()
        let old = try create("Old planning", at: now.addingTimeInterval(-40 * 86_400))
        let medium = try create("Older review", at: now.addingTimeInterval(-20 * 86_400))
        let recent = try create("Recent release", at: now.addingTimeInterval(-2 * 86_400))
        let running = try create("Running investigation", at: now.addingTimeInterval(-40 * 86_400))
        let worklog = try start(running, at: now)
        return ArchiveSeed(old: old, medium: medium, recent: recent, running: running, worklog: worklog)
    }
}
