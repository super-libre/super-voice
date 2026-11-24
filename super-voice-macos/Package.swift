// swift-tools-version:6.0
// The SwiftUI app for macOS, on top of super-voice-app-core.
//
// `just macos-swift-app` builds it: first the core as a static library and the
// Swift bindings UniFFI generates over it (both into Generated/, which git
// ignores), then this package, then the app bundle.
import PackageDescription

let package = Package(
    name: "SuperVoice",
    platforms: [.macOS(.v14)],
    targets: [
        // The core as a static library, with the C header UniFFI generates.
        .binaryTarget(
            name: "SuperVoiceFFI",
            path: "Generated/SuperVoiceFFI.xcframework"
        ),
        // The Swift bindings UniFFI generates over that library. Swift 5 mode:
        // the generated code predates Swift 6's strict concurrency checks.
        .target(
            name: "SuperVoiceCore",
            dependencies: ["SuperVoiceFFI"],
            path: "Generated/SuperVoiceCore",
            swiftSettings: [.swiftLanguageMode(.v5)],
            // What the core's library calls into. The audio frameworks are
            // for cpal, the core's recorder and player: a release build
            // strips that code when the app doesn't use it, a debug build
            // keeps every object and needs them.
            linkerSettings: [
                .linkedFramework("AudioToolbox"),
                .linkedFramework("CoreAudio"),
                .linkedFramework("CoreFoundation"),
                .linkedFramework("Security"),
                .linkedFramework("SystemConfiguration"),
            ]
        ),
        .executableTarget(
            name: "SuperVoice",
            dependencies: ["SuperVoiceCore"],
            path: "Sources/SuperVoice"
        ),
    ]
)
