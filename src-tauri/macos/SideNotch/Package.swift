// swift-tools-version: 5.9

import Foundation
import PackageDescription

let info = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appendingPathComponent(
  "Info.plist"
).path
let concurrencyChecks: [SwiftSetting] = [.enableExperimentalFeature("StrictConcurrency")]

let package = Package(
  name: "SideNotch",
  platforms: [.macOS(.v11)],
  products: [.executable(name: "on-n-off-notch", targets: ["NotchApp"])],
  targets: [
    .target(name: "NotchCore", swiftSettings: concurrencyChecks),
    .executableTarget(
      name: "NotchApp", dependencies: ["NotchCore"],
      swiftSettings: concurrencyChecks,
      linkerSettings: [
        .unsafeFlags([
          "-Xlinker", "-sectcreate", "-Xlinker", "__TEXT", "-Xlinker", "__info_plist", "-Xlinker",
          info,
        ])
      ]),
    .executableTarget(
      name: "NotchCoreChecks", dependencies: ["NotchCore"], path: "Tests/NotchCoreTests",
      swiftSettings: concurrencyChecks),
  ]
)
