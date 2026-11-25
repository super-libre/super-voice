// SPDX-License-Identifier: GPL-3.0-only
// The microphone, as the daemon reports it: open or not, its level, and
// whether that sounds like speech. Pages that record read it.

import SuperVoiceCore
import SwiftUI

@MainActor
final class MicrophoneModel: ObservableObject {
    static let shared = MicrophoneModel()

    @Published private(set) var microphone = Microphone(recording: false, level: 0, speech: false)

    private var store: MicrophoneStore!

    private init() {
        let relay = MicrophoneRelay { [weak self] microphone in
            Task { @MainActor in self?.microphone = microphone }
        }
        store = MicrophoneStore(observer: relay)
    }

    #if DEBUG
    /// Shows `microphone` as if the daemon had said it: the take scenes'.
    func pretend(_ microphone: Microphone) {
        self.microphone = microphone
    }
    #endif
}

/// Hands the store's calls, which come on its own threads, to the model.
final class MicrophoneRelay: MicrophoneObserver {
    let onChange: @Sendable (Microphone) -> Void

    init(onChange: @escaping @Sendable (Microphone) -> Void) {
        self.onChange = onChange
    }

    func changed(microphone: Microphone) {
        onChange(microphone)
    }
}
