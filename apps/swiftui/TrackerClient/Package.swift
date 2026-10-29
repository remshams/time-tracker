// swift-tools-version: 6.4
import PackageDescription

let approachableConcurrency: [SwiftSetting] = [
    .enableUpcomingFeature("NonisolatedNonsendingByDefault"),
    .enableUpcomingFeature("InferIsolatedConformances"),
]

let package = Package(
    name: "TrackerClient",
    platforms: [.macOS(.v27)],
    products: [.library(name: "TrackerClient", targets: ["TrackerClient"])],
    targets: [
        .target(name: "TrackerClient", swiftSettings: [.defaultIsolation(MainActor.self)] + approachableConcurrency),
        // Default isolation would make XCTest's inherited nonisolated initializer main-actor, which doesn't compile.
        // Tests opt into the main actor explicitly.
        .testTarget(
            name: "TrackerClientTests", dependencies: ["TrackerClient"], swiftSettings: approachableConcurrency),
    ],
    swiftLanguageModes: [.v6]
)
