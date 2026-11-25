// SPDX-License-Identifier: GPL-3.0-only
// The Voices page's model: the core's store underneath, the page it last drew,
// and the fields the user types into.

import AppKit
import SuperVoiceCore
import SwiftUI
import UniformTypeIdentifiers

@MainActor
final class VoicesModel: ObservableObject {
    static let shared = VoicesModel()

    /// What the store last said the page shows.
    @Published private(set) var page = VoicesPage(
        loaded: false, voices: [], model: nil, language: nil, scripts: [], script: nil,
        ownWords: false, recording: nil, pending: nil, label: "", labelReplaced: 0,
        transcript: "", transcriptReplaced: 0, renaming: nil, renameLabel: "", previewing: nil,
        notice: nil
    )

    /// The new voice's name and transcript. The store gets each change, and
    /// replaces them only when it fills or empties them itself: a name from
    /// the file imported, a script's words, both emptied once it is saved.
    @Published var label = ""
    @Published var transcript = ""

    /// The name a voice is being renamed to.
    @Published var renameLabel = ""

    private(set) var store: VoicesStore!
    private var labelReplaced: UInt64 = 0
    private var transcriptReplaced: UInt64 = 0

    private init() {
        let relay = VoicesRelay(
            onChange: { [weak self] page in Task { @MainActor in self?.show(page) } },
            onFileRequested: { [weak self] in Task { @MainActor in self?.askForFile() } }
        )
        store = VoicesStore(observer: relay)
    }

    func send(_ action: VoicesAction) {
        store.send(action: action)
    }

    private func show(_ page: VoicesPage) {
        if page.renaming != nil, page.renaming != self.page.renaming {
            renameLabel = page.renameLabel
        }
        self.page = page
        if page.labelReplaced != labelReplaced {
            labelReplaced = page.labelReplaced
            label = page.label
        }
        if page.transcriptReplaced != transcriptReplaced {
            transcriptReplaced = page.transcriptReplaced
            transcript = page.transcript
        }
        #if DEBUG
        Snapshot.voicesShown(page)
        #endif
    }

    /// Ask for a WAV file to make a voice from. Only WAV: that is what the
    /// daemon keeps.
    private func askForFile() {
        let panel = NSOpenPanel()
        panel.title = t("choose-a-voice-recording")
        panel.allowedContentTypes = [.wav]
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        let store = store!
        panel.begin { answer in
            store.filePicked(path: answer == .OK ? panel.url?.path : nil)
        }
    }
}

/// Hands the store's calls, which come on its own threads, to the model.
final class VoicesRelay: VoicesObserver {
    let onChange: @Sendable (VoicesPage) -> Void
    let onFileRequested: @Sendable () -> Void

    init(
        onChange: @escaping @Sendable (VoicesPage) -> Void,
        onFileRequested: @escaping @Sendable () -> Void
    ) {
        self.onChange = onChange
        self.onFileRequested = onFileRequested
    }

    func changed(page: VoicesPage) {
        onChange(page)
    }

    func fileRequested() {
        onFileRequested()
    }
}
