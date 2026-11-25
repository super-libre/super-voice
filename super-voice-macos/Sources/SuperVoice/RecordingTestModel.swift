// SPDX-License-Identifier: GPL-3.0-only
// The recording test's model: the core's store underneath, and the test it
// last drew.

import SuperVoiceCore
import SwiftUI

@MainActor
final class RecordingTestModel: ObservableObject {
    static let shared = RecordingTestModel()

    /// What the store last said the test shows.
    @Published private(set) var page = RecordingTestPage(
        recording: false, speech: false, level: 0, preview: "", previewSpans: nil, heard: nil
    )

    private var store: RecordingTestStore!

    private init() {
        let relay = RecordingTestRelay { [weak self] page in Task { @MainActor in self?.show(page) } }
        store = RecordingTestStore(observer: relay)
    }

    private func show(_ page: RecordingTestPage) {
        self.page = page
        #if DEBUG
        Snapshot.recordingTestShown(page)
        #endif
    }

    func send(_ action: RecordingTestAction) {
        store.send(action: action)
    }
}

/// Hands the store's calls, which come on its own threads, to the model.
final class RecordingTestRelay: RecordingTestObserver {
    let onChange: @Sendable (RecordingTestPage) -> Void

    init(onChange: @escaping @Sendable (RecordingTestPage) -> Void) {
        self.onChange = onChange
    }

    func changed(page: RecordingTestPage) {
        onChange(page)
    }
}
