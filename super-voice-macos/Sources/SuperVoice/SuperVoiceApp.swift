// SPDX-License-Identifier: GPL-3.0-only
// The SwiftUI app for macOS: one window, laid out like the settings app on
// Linux, with its pages arriving one at a time.

import AppKit
import SwiftUI

@main
struct SuperVoiceApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    init() {
        chooseLanguages()
    }

    var body: some Scene {
        WindowGroup("Super Voice") {
            ContentView(notes: NotesModel.shared)
                .frame(minWidth: 720, minHeight: 480)
        }
    }
}

/// The app quitting: the notes not yet written are written first.
final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationWillTerminate(_ notification: Notification) {
        MainActor.assumeIsolated {
            NotesModel.shared.saveBeforeExit()
        }
    }
}
