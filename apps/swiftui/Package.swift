// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "TimeTrackerSwiftUI",
    platforms: [.macOS(.v13)],
    products: [.executable(name: "TimeTrackerSwiftUI", targets: ["TimeTrackerSwiftUI"])],
    targets: [
        .target(name: "CTrackerBridge", publicHeadersPath: "include"),
        .executableTarget(
            name: "TimeTrackerSwiftUI",
            dependencies: ["CTrackerBridge"],
            linkerSettings: [
                .linkedLibrary("tracker_swift_bridge"),
                .linkedFramework("Security"),
                .linkedFramework("CoreFoundation"),
                .linkedFramework("SystemConfiguration")
            ]
        )
    ]
)
