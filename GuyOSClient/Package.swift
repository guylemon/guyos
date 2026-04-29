// swift-tools-version: 6.3
// The swift-tools-version declares the minimum version of Swift required to build this package.

import PackageDescription

let package = Package(
    name: "GuyOSClient",
    platforms: [
        .iOS(.v13)
    ],
    products: [
        .library(
            name: "GuyOSClient",
            targets: ["GuyOSClient"]
        )
    ],
    targets: [
        .target(
            name: "GuyOSClient",
            dependencies: ["guyos_coreFFI"],
            path: "Sources/GuyOSClient"
        ),
        .binaryTarget(
            name: "guyos_coreFFI",
            path: "GuyOSClient.xcframework"
        )
    ]
)
