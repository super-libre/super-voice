// SPDX-License-Identifier: GPL-3.0-only
// Speech to text's Behavior tab: what stops a recording and whether its words
// are typed as they are heard, how they are typed, the sounds around it, and
// the tests that record and type. As in the settings app.

import SuperVoiceCore
import SwiftUI

struct SttBehaviorView: View {
    @ObservedObject var model: SttBehaviorModel
    @FocusState private var typingFieldFocused: Bool

    var body: some View {
        let page = model.page
        VStack(spacing: 0) {
            if let notice = page.notice {
                NoticeBanner(text: notice.message)
            }
            Form {
                Section {
                    Picker(selection: choice(page.stopMode) { .stopModeChanged(id: $0) }) {
                        ForEach(page.stopModes, id: \.id) { Text($0.name).tag($0.id) }
                    } label: {
                        Labelled(t("stop-mode"), t("controls-how-to-stop-transcribing"))
                    }
                    Toggle(isOn: toggle(page.previewTyping) { .previewTypingToggled(on: $0) }) {
                        Labelled(t("preview-typing"), t("shows-transcription-as-you-speak-experimental-may"))
                    }
                } header: {
                    Text(t("recording"))
                }

                Section {
                    Picker(selection: choice(page.writeMethod) { .writeMethodChanged(id: $0) }) {
                        ForEach(page.writeMethods, id: \.id) { Text($0.name).tag($0.id) }
                    } label: {
                        Labelled(t("write-method"), t("controls-how-transcribed-text-is-typed-into"))
                    }
                    // Auto names a chain, not a method: only a test says which
                    // one typed.
                    if let typedWith = page.typedWith {
                        LabeledContent {
                            Text(typedWith)
                        } label: {
                            Labelled(t("active-backend"), t("what-the-last-test-actually-typed-through"))
                        }
                    }
                } header: {
                    Text(t("typing"))
                }

                Section {
                    Toggle(isOn: toggle(page.soundsOn) { .soundsToggled(on: $0) }) {
                        Labelled(t("audio-feedback"), t("play-a-sound-when-a-recording-starts-and-stops"))
                    }
                    if page.soundsOn {
                        Picker(selection: choice(page.theme) { .themePicked(id: $0) }) {
                            ForEach(page.themes, id: \.id) { Text($0.name).tag($0.id) }
                        } label: {
                            Labelled(t("theme"), t("the-sounds-a-recording-plays"))
                        }
                        LabeledContent(t("volume")) {
                            HStack {
                                Text("\(Int(model.volume.rounded()))%")
                                    .monospacedDigit()
                                    .foregroundStyle(.secondary)
                                Slider(value: $model.volume, in: 0...100) { editing in
                                    // Stored once, when the slider is let go.
                                    if !editing {
                                        model.send(.volumeCommitted)
                                    }
                                }
                                .frame(width: 220)
                                .onChange(of: model.volume) { _, volume in
                                    model.send(.volumeChanged(volume: UInt8(volume.rounded())))
                                }
                            }
                        }
                    }
                } header: {
                    Text(t("sounds"))
                }

                Section {
                    RecordingTestRows()
                    VStack(alignment: .leading, spacing: 6) {
                        Labelled(t("typing-test"), t("types-a-short-string-into-the-field"))
                        HStack {
                            TextField(t("typing-test"), text: $model.typingText, prompt: Text(t("typed-text-lands-here-progress")))
                                .labelsHidden()
                                .focused($typingFieldFocused)
                                .onChange(of: model.typingText) { _, text in
                                    model.send(.typingTestTextChanged(text: text))
                                }
                            Button(t("type-test-text")) { model.send(.testTyping) }
                                .buttonStyle(.borderedProminent)
                        }
                    }
                    LabeledContent {
                        // Mid-countdown the only thing to do is call it off: a
                        // second test would race the first into whichever
                        // window has focus.
                        if let seconds = page.typingCountdown {
                            Button(t("cancel-typing-countdown", ["secs": "\(seconds)"])) {
                                model.send(.cancelTypingTest)
                            }
                            .buttonStyle(.borderedProminent)
                            .tint(.red)
                        } else {
                            Button(t("start-countdown")) { model.send(.testTypingLater) }
                        }
                    } label: {
                        Labelled(t("test-another-window"), t("counts-down-before-typing-so-you-can"))
                    }
                } header: {
                    Text(t("test"))
                }
            }
            .formStyle(.grouped)
        }
        .onAppear { model.send(.reload) }
        .onChange(of: model.focusAsked) { _, _ in typingFieldFocused = true }
    }

    /// A picker's value: the store's choice, and what picking another sends.
    private func choice(_ current: String, _ action: @escaping (String) -> BehaviorAction) -> Binding<String> {
        Binding(get: { current }, set: { model.send(action($0)) })
    }

    /// A switch's value: the store's, and what flipping it sends. It moves
    /// once the daemon has stored it.
    private func toggle(_ current: Bool, _ action: @escaping (Bool) -> BehaviorAction) -> Binding<Bool> {
        Binding(get: { current }, set: { model.send(action($0)) })
    }
}

/// A setting's name with its explanation under it.
struct Labelled: View {
    let title: String
    let detail: String

    init(_ title: String, _ detail: String) {
        self.title = title
        self.detail = detail
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title)
            Text(detail)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }
}

/// The recording test: what the microphone hears, the button that starts and
/// stops a take, its level, its words as they come, then its transcript.
struct RecordingTestRows: View {
    @ObservedObject var model = RecordingTestModel.shared

    var body: some View {
        let page = model.page
        LabeledContent(t("status")) {
            HStack {
                Text(page.recording ? (page.speech ? t("speech") : t("silence")) : t("not-recording"))
                if page.recording {
                    Button(t("stop-recording"), role: .destructive) { model.send(.stop) }
                        .buttonStyle(.borderedProminent)
                        .tint(.red)
                } else {
                    Button(t("test-recording")) { model.send(.start) }
                        .buttonStyle(.borderedProminent)
                }
            }
        }
        LabeledContent(t("audio-level")) {
            // A floor under any sound at all, so quiet speech still moves the
            // meter.
            ProgressView(value: Double(page.level > 0 ? max(page.level, 0.1) : 0))
                .progressViewStyle(.linear)
                .frame(width: 240)
        }
        // The newest words on one line, their end showing, so the rows don't
        // move as they come; what they span goes under the row's name.
        HStack(alignment: .firstTextBaseline) {
            if let spans = page.previewSpans {
                Labelled(t("live"), spans.words)
            } else {
                Text(t("live"))
            }
            Spacer(minLength: 24)
            Text(page.preview.isEmpty ? "\u{2014}" : page.preview)
                .lineLimit(1)
                .truncationMode(.head)
                .foregroundStyle(page.preview.isEmpty ? .secondary : .primary)
        }
        ScrollView {
            Text(transcript(page.heard))
                .foregroundStyle(page.heard == nil ? .secondary : .primary)
                .frame(maxWidth: .infinity, alignment: .leading)
                .textSelection(.enabled)
        }
        .frame(height: 66)
    }

    private func transcript(_ heard: TestHeard?) -> String {
        switch heard {
        case nil: t("transcriptions-will-appear-here-after-test-recordings")
        case let .words(text): text
        case .nothing: t("no-speech-detected")
        case let .failed(message): t("the-recording-failed", ["error": message])
        }
    }
}
