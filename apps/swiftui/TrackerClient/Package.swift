// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "TrackerClient",
    platforms: [.macOS(.v13)],
    products: [.library(name: "TrackerClient", targets: ["TrackerClient"])],
    targets: [
        .target(name: "TrackerClient"),
        .testTarget(name: "TrackerClientTests", dependencies: ["TrackerClient"]),
    ],
    swiftLanguageVersions: [.v5]
)
