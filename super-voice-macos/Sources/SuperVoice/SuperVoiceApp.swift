// SPDX-License-Identifier: GPL-3.0-only
// The SwiftUI app for macOS. An early version: one window, the Notes list,
// read through the core.

import SwiftUI

@main
struct SuperVoiceApp: App {
    var body: some Scene {
        WindowGroup("Super Voice") {
            NotesView()
                .frame(minWidth: 480, minHeight: 360)
        }
    }
}
