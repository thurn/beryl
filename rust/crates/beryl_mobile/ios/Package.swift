// swift-tools-version:5.5

import PackageDescription

let package = Package(
    name: "beryl-mobile",
    platforms: [.iOS(.v13)],
    products: [.library(name: "beryl-mobile", type: .static, targets: ["BerylMobile"])],
    dependencies: [.package(name: "Tauri", path: "../.tauri/tauri-api")],
    targets: [.target(name: "BerylMobile", dependencies: ["Tauri"], path: "Sources")]
)
