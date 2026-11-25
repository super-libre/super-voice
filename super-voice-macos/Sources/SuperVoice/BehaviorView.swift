// SPDX-License-Identifier: GPL-3.0-only
// Text to speech's Behavior tab. On Linux it also holds Read aloud, which
// reads the Wayland session; here it is the speech test: text to speak, what
// the daemon is saying, and how loud.

import SuperVoiceCore
import SwiftUI

struct TextToSpeechBehaviorView: View {
    @ObservedObject var model: SpeechModel

    var body: some View {
        VStack(spacing: 0) {
            if let notice = model.page.notice {
                NoticeBanner(text: notice.words)
            }
            Form {
                Section {
                    LabeledContent(t("status")) {
                        HStack(spacing: 12) {
                            Text(status)
                            if model.page.speaking {
                                Button(t("stop")) { model.send(.stop) }
                                    .buttonStyle(.borderedProminent)
                                    .tint(.red)
                            } else {
                                Button(t("speak")) { model.send(.speak) }
                                    .buttonStyle(.borderedProminent)
                                    .disabled(!model.page.canSpeak)
                            }
                        }
                    }
                    TextField(t("text"), text: $model.text, prompt: Text(t("type-something-to-hear-it")))
                        .onChange(of: model.text) { _, text in model.textChanged(text) }
                        .onSubmit {
                            if model.page.canSpeak {
                                model.send(.speak)
                            }
                        }
                    LabeledContent(t("output-level")) {
                        // A floor under any sound at all, so quiet speech
                        // still moves the meter.
                        ProgressView(value: Double(model.page.level > 0 ? max(model.page.level, 0.1) : 0))
                            .progressViewStyle(.linear)
                            .frame(width: 240)
                    }
                } header: {
                    Text(t("test"))
                }
            }
            .formStyle(.grouped)
        }
        #if DEBUG
        .onAppear { Snapshot.speechShown(model.page) }
        #endif
    }

    /// What the daemon is doing: an utterance it took is speaking before its
    /// first sound plays.
    private var status: String {
        guard model.page.speaking else { return t("not-speaking-status") }
        return model.page.playing ? t("speaking-status") : t("starting-status")
    }
}

extension SpeechNotice {
    /// What the test says about it: the daemon's own words, as in the
    /// settings app.
    var words: String {
        switch self {
        case let .couldNotSpeak(message): message
        }
    }
}
