// SPDX-License-Identifier: GPL-3.0-only
// The Voices tab: what the loaded model can do with a cloned voice; a script
// to read and the microphone, or a WAV file, to add one; then the voices
// saved, each to hear, rename or delete. As in the settings app.

import SuperVoiceCore
import SwiftUI

struct VoicesView: View {
    @ObservedObject var model: VoicesModel

    var body: some View {
        VStack(spacing: 0) {
            if let notice = model.page.notice {
                NoticeBanner(text: notice.words)
            }
            Form {
                Section {
                    ModelNote(model: model.page.model)
                } header: {
                    Text(t("active-model-title"))
                }

                if let recording = model.page.recording {
                    RecordingSection(model: model, recording: recording)
                } else if let pending = model.page.pending {
                    PendingSection(model: model, pending: pending)
                } else {
                    AddSection(model: model)
                }

                Section {
                    if !model.page.loaded {
                        Text(t("reading-the-library"))
                            .foregroundStyle(.secondary)
                    } else if model.page.voices.isEmpty {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(t("no-voices-yet"))
                            Text(t("record-a-voice-or-import-a-recording"))
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                    ForEach(model.page.voices, id: \.id) { voice in
                        VoiceRowView(model: model, voice: voice)
                    }
                } header: {
                    Text(t("saved-voices"))
                }
            }
            .formStyle(.grouped)
        }
        .onAppear { model.send(.refresh) }
    }
}

/// What the loaded model can do with these voices: said once, at the top,
/// since it is the model's, not any one voice's.
struct ModelNote: View {
    let model: VoiceModel?

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(status)
                .font(.headline)
            Text(detail)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }

    private var status: String {
        guard let model else { return t("no-model-loaded") }
        return model.clones
            ? t("model-clones-voices", ["model": model.name])
            : t("model-does-not-clone", ["model": model.name])
    }

    private var detail: String {
        guard let model else { return t("voices-you-add-are-kept") }
        guard model.clones else { return t("this-model-speaks-only-in-its-own-preset-voices") }
        let budget = model.cloneRefSeconds.map {
            t("up-to-seconds-of-reference-audio", ["seconds": whole($0)])
        } ?? t("any-length-of-reference-audio")
        var detail = t("it-uses-budget-from-each-recording", ["budget": budget])
        if model.needsTranscript {
            detail += " " + t("it-also-needs-the-transcript")
        }
        return detail
    }
}

/// The two ways to add a voice: read a script into the microphone, or import
/// a recording.
struct AddSection: View {
    @ObservedObject var model: VoicesModel

    /// The picker's value for reading your own words.
    private static let ownWords = ""

    var body: some View {
        let page = model.page
        Section {
            Picker(selection: scriptChoice) {
                Text(t("no-script-read-your-own-words")).tag(Self.ownWords)
                ForEach(page.scripts, id: \.id) { script in
                    Text("\(languageName(script.language)) · \(script.title)").tag(script.id)
                }
            } label: {
                VStack(alignment: .leading, spacing: 2) {
                    Text(t("script"))
                    Text(pickerNote(page.script, language: page.language))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            if let script = page.script {
                ScriptCard(script: script, size: 15, meta: meta(script, model: page.model))
            }
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(t("new-recording"))
                    Text(page.script == nil ? t("read-a-couple-of-sentences") : t("press-record-and-read-the-script"))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button(t("record")) { model.send(.startRecording) }
                    .buttonStyle(.borderedProminent)
                Button(t("import-a-wav-file")) { model.send(.importFile) }
            }
        } header: {
            Text(t("add-a-voice"))
        }
    }

    /// The picker's value: a script's id, or `ownWords`.
    private var scriptChoice: Binding<String> {
        Binding(
            get: { model.page.ownWords ? Self.ownWords : (model.page.script?.id ?? Self.ownWords) },
            set: { model.send(.scriptSelected(id: $0 == Self.ownWords ? nil : $0)) }
        )
    }

    /// The line under the picker: what the script is for, and whether it is
    /// in the language the voice will speak.
    private func pickerNote(_ script: ScriptInfo?, language: String?) -> String {
        guard let script else { return t("nothing-to-read") }
        if !script.inLanguage, let language {
            return t("script-language-note", [
                "blurb": script.blurb,
                "script_language": languageName(script.language),
                "speak_language": languageName(language),
            ])
        }
        return script.blurb
    }

    /// How long a script takes to read, and a warning when the loaded model
    /// can't hear all of it.
    private func meta(_ script: ScriptInfo, model: VoiceModel?) -> String {
        let pace = t("about-seconds-at-an-unhurried-pace", ["seconds": whole(script.seconds)])
        if !script.fits, let budget = model?.cloneRefSeconds {
            return t("script-longer-than-budget", ["pace": pace, "budget": whole(budget)])
        }
        return pace
    }
}

/// While the microphone is open: the script, larger, to read from a chair's
/// distance; the time; the input level; and Stop.
struct RecordingSection: View {
    @ObservedObject var model: VoicesModel
    let recording: Recording

    var body: some View {
        Section {
            if let script = model.page.script {
                ScriptCard(script: script, size: 18, meta: nil)
            }
            LabeledContent(t("elapsed")) {
                Text(t("seconds-of-seconds", [
                    "elapsed": whole(recording.seconds),
                    "max": whole(recording.maxSeconds),
                ]))
                .monospacedDigit()
            }
            LabeledContent(t("input-level")) {
                HStack {
                    // A floor under any sound at all, so a quiet room still
                    // moves the meter: one that reads flat looks like a dead
                    // microphone.
                    ProgressView(value: Double(recording.level > 0 ? max(recording.level, 0.05) : 0))
                        .progressViewStyle(.linear)
                        .frame(width: 240)
                    Button(t("stop")) { model.send(.stopRecording) }
                        .buttonStyle(.borderedProminent)
                        .tint(.red)
                }
            }
        } header: {
            Text(t("recording"))
        }
    }
}

/// A recording waiting to be named and saved.
struct PendingSection: View {
    @ObservedObject var model: VoicesModel
    let pending: PendingVoice

    var body: some View {
        Section {
            LabeledContent(t("recording")) {
                Text(summary)
                    .monospacedDigit()
            }
            TextField(t("name"), text: $model.label, prompt: Text(t("name-this-voice")))
                .onChange(of: model.label) { _, label in model.send(.labelChanged(label: label)) }
            VStack(alignment: .leading, spacing: 6) {
                Text(t("transcript"))
                    .font(.headline)
                if let script = pending.script {
                    // A built-in script's words are the words read: shown,
                    // not offered for editing.
                    Text(t("from-the-built-in-script"))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    ScriptCard(script: script, size: 15, meta: nil)
                } else {
                    Text(model.page.model?.needsTranscript == true
                        ? t("required-by-the-loaded-model")
                        : t("optional-some-models-clone-from-the-words"))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    TextField(t("transcript"), text: $model.transcript, prompt: Text(t("what-the-recording-says")), axis: .vertical)
                        .labelsHidden()
                        .lineLimit(2...6)
                        .onChange(of: model.transcript) { _, text in
                            model.send(.transcriptChanged(transcript: text))
                        }
                }
            }
            HStack {
                Spacer()
                Button(pending.saving ? t("saving-progress") : t("save-voice")) { model.send(.save) }
                    .buttonStyle(.borderedProminent)
                    .disabled(!pending.canSave)
                Button(t("discard")) { model.send(.discardPending) }
                    .buttonStyle(.borderedProminent)
                    .tint(.red)
                    .disabled(pending.saving)
            }
        } header: {
            Text(t("new-voice"))
        }
    }

    /// Where it came from, and how long it is.
    private var summary: String {
        let origin = pending.importedFrom.map { t("imported-from", ["name": $0]) } ?? t("recorded-just-now")
        guard pending.seconds > 0 else { return origin }
        return origin + " · " + String(format: "%.1f s", pending.seconds)
    }
}

/// A script, as a block to read from.
struct ScriptCard: View {
    let script: ScriptInfo
    let size: CGFloat
    let meta: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(script.text)
                .font(.system(size: size))
                .lineSpacing(size * 0.3)
                .frame(maxWidth: .infinity, alignment: .leading)
                .textSelection(.enabled)
            if let meta {
                Text(meta)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(12)
        .background(Color(nsColor: .quaternarySystemFill), in: RoundedRectangle(cornerRadius: 8))
    }
}

/// One saved voice: its facts, and its buttons. Renaming swaps them for a
/// field.
struct VoiceRowView: View {
    @ObservedObject var model: VoicesModel
    let voice: VoiceRow

    var body: some View {
        if model.page.renaming == voice.id {
            HStack {
                TextField(t("name"), text: $model.renameLabel, prompt: Text(t("name-this-voice")))
                    .labelsHidden()
                    .onSubmit(commit)
                Button(t("save"), action: commit)
                    .buttonStyle(.borderedProminent)
                Button(t("cancel")) { model.send(.cancelRename) }
            }
        } else {
            HStack(spacing: 8) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(voice.label)
                    Text(about)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .textSelection(.enabled)
                }
                Spacer()
                // Only when the loaded model can speak in it; one at a time,
                // since a second preview cuts off the first.
                if model.page.model?.clones == true {
                    Button(previewing ? t("preparing-progress") : t("preview")) {
                        model.send(.preview(voiceId: voice.voiceId))
                    }
                    .disabled(model.page.previewing != nil)
                }
                if !voice.deleting {
                    Button(t("rename")) { model.send(.beginRename(id: voice.id, label: voice.label)) }
                    Button(t("delete")) { model.send(.delete(id: voice.id)) }
                        .buttonStyle(.borderedProminent)
                        .tint(.red)
                }
            }
            .padding(.vertical, 2)
        }
    }

    private var previewing: Bool {
        model.page.previewing == voice.voiceId
    }

    /// How long its recording is, whether it has a transcript, and the id an
    /// app speaks in it with; or what is happening to it.
    private var about: String {
        if voice.deleting {
            return t("deleting-progress")
        }
        if previewing {
            return t("getting-the-model-ready")
        }
        return t("voice-meta", [
            "seconds": String(format: "%.1f", voice.seconds),
            "transcript": voice.hasTranscript ? t("with-transcript") : t("no-transcript"),
            "id": voice.voiceId,
        ])
    }

    private func commit() {
        model.send(.renameChanged(label: model.renameLabel))
        model.send(.commitRename)
    }
}

extension VoicesNotice {
    /// What the page says about it. The daemon's and the device's own
    /// messages are shown as they come, as in the settings app.
    var words: String {
        switch self {
        case let .couldNotLoad(message), let .couldNotImport(message),
             let .couldNotRename(message), let .couldNotDelete(message),
             let .couldNotPreview(message), let .couldNotRecord(message),
             let .couldNotSave(message):
            message
        case .needsTranscript: t("this-voice-needs-a-transcript")
        }
    }
}

/// A language's name, from its tag, in the app's language.
@MainActor func languageName(_ tag: String) -> String {
    guard let name = appLocale.localizedString(forIdentifier: tag) else { return tag }
    return name.prefix(1).uppercased(with: appLocale) + name.dropFirst()
}

/// `seconds` as a whole number, as the page says lengths.
func whole(_ seconds: Float) -> String {
    String(Int(seconds.rounded()))
}
