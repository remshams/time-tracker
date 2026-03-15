import Foundation
// swift-tools-version: 6.0
import PackageDescription

func shellOutput(_ executable: String, arguments: [String]) -> String? {
    let process = Process()
    let outputPipe = Pipe()

    process.executableURL = URL(fileURLWithPath: executable)
    process.arguments = arguments
    process.standardOutput = outputPipe
    process.standardError = Pipe()

    do {
        try process.run()
        process.waitUntilExit()
    } catch {
        return nil
    }

    guard process.terminationStatus == 0 else {
        return nil
    }

    let outputData = outputPipe.fileHandleForReading.readDataToEndOfFile()
    return String(data: outputData, encoding: .utf8)?
        .trimmingCharacters(in: .whitespacesAndNewlines)
}

let developerDir =
    ProcessInfo.processInfo.environment["DEVELOPER_DIR"]
    ?? shellOutput("/usr/bin/xcode-select", arguments: ["-p"])

let testingFrameworkPathCandidates = [
    developerDir.map { "\($0)/Platforms/MacOSX.platform/Developer/Library/Frameworks" },
    developerDir.map { "\($0)/Library/Developer/Frameworks" },
].compactMap { $0 }
let testingFrameworkPath = testingFrameworkPathCandidates.first {
    FileManager.default.fileExists(atPath: "\($0)/Testing.framework")
}

let package = Package(
    name: "App",
    platforms: [
        .macOS(.v13)
    ],
    products: [
        .executable(name: "App", targets: ["App"])
    ],
    targets: [
        .executableTarget(
            name: "App"
        ),
        .testTarget(
            name: "AppTests",
            dependencies: ["App"],
            swiftSettings: testingFrameworkPath.map {
                [.unsafeFlags(["-F", $0], .when(platforms: [.macOS]))]
            },
            linkerSettings: testingFrameworkPath.map {
                [
                    .unsafeFlags(
                        [
                            "-Xlinker", "-F",
                            "-Xlinker", $0,
                            "-Xlinker", "-rpath",
                            "-Xlinker", $0,
                        ], .when(platforms: [.macOS])),
                    .linkedFramework("Testing", .when(platforms: [.macOS])),
                ]
            }
        ),
    ]
)
