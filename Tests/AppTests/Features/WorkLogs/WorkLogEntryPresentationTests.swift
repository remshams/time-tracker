import Foundation
import Testing

@testable import App

@Test func formattedTimeRangeShowsStartAndEndTime() {
    let startedAt = Date(timeIntervalSince1970: 1_700_000_000)
    let endedAt = Date(timeIntervalSince1970: 1_700_000_900)
    let entry = TestFactories.makeWorkLogEntry(
        taskID: TestFactories.anyTaskID,
        startedAt: startedAt,
        endedAt: endedAt)

    let expectedStart = startedAt.formatted(date: .omitted, time: .shortened)
    let expectedEnd = endedAt.formatted(date: .omitted, time: .shortened)

    #expect(entry.formattedTimeRange == "\(expectedStart) – \(expectedEnd)")
}

@Test func formattedTimeRangeShowsRunningWhenEndedAtIsNil() {
    let startedAt = Date(timeIntervalSince1970: 1_700_000_000)
    let entry = TestFactories.makeWorkLogEntry(
        taskID: TestFactories.anyTaskID,
        startedAt: startedAt,
        endedAt: nil)

    let expectedStart = startedAt.formatted(date: .omitted, time: .shortened)

    #expect(entry.formattedTimeRange == "\(expectedStart) – Running")
}

@Test func formattedDurationShowsHoursAndMinutes() {
    let startedAt = Date(timeIntervalSince1970: 1_700_000_000)
    let endedAt = Date(timeIntervalSince1970: 1_700_005_400)  // 1h 30m
    let entry = TestFactories.makeWorkLogEntry(
        taskID: TestFactories.anyTaskID,
        startedAt: startedAt,
        endedAt: endedAt)

    #expect(entry.formattedDuration == "1h 30m")
}

@Test func formattedDurationShowsMinutesOnlyWhenUnderOneHour() {
    let startedAt = Date(timeIntervalSince1970: 1_700_000_000)
    let endedAt = Date(timeIntervalSince1970: 1_700_002_700)  // 45m
    let entry = TestFactories.makeWorkLogEntry(
        taskID: TestFactories.anyTaskID,
        startedAt: startedAt,
        endedAt: endedAt)

    #expect(entry.formattedDuration == "45m")
}

@Test func formattedDurationShowsSecondsWhenUnderOneMinute() {
    let startedAt = Date(timeIntervalSince1970: 1_700_000_000)
    let endedAt = Date(timeIntervalSince1970: 1_700_000_045)  // 45s
    let entry = TestFactories.makeWorkLogEntry(
        taskID: TestFactories.anyTaskID,
        startedAt: startedAt,
        endedAt: endedAt)

    #expect(entry.formattedDuration == "45s")
}

@Test func formattedDurationShowsDashWhenEndedAtIsNil() {
    let entry = TestFactories.makeWorkLogEntry(
        taskID: TestFactories.anyTaskID,
        endedAt: nil)

    #expect(entry.formattedDuration == "–")
}

@Test func formattedDurationShowsZeroMinutesWhenDurationIsExactHours() {
    let startedAt = Date(timeIntervalSince1970: 1_700_000_000)
    let endedAt = Date(timeIntervalSince1970: 1_700_007_200)  // exactly 2h
    let entry = TestFactories.makeWorkLogEntry(
        taskID: TestFactories.anyTaskID,
        startedAt: startedAt,
        endedAt: endedAt)

    #expect(entry.formattedDuration == "2h 0m")
}
