// swift-tools-version: 6.4
import PackageDescription

let package = Package(
    name: "TrackerClient",
    platforms: [.macOS(.v27)],
    products: [.library(name: "TrackerClient", targets: ["TrackerClient"])],
    targets: [
        .target(name: "TrackerClient"),
        .testTarget(name: "TrackerClientTests", dependencies: ["TrackerClient"]),
    ],
    swiftLanguageModes: [.v5]
)
