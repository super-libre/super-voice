// SPDX-License-Identifier: GPL-3.0-only
// A model's language and the synthesis model's voice, as the Models card shows
// them: the core's store underneath, the controls, and their notices. As in
// the settings app.
//
// Each control focuses the model it is given, which reads its language (and
// for synthesis its voice) from the daemon. The store follows the daemon's
// events, so a voice recorded elsewhere, or a primary language changed, shows
// here too.

import SuperVoiceCore
import SwiftUI

@MainActor
final class ModelDetailsModel: ObservableObject {
    static let shared = ModelDetailsModel()

    /// What the store last said the cards show.
    @Published private(set) var page = ModelDetailsPage(
        languages: [],
        voice: ModelVoiceRow(source: "", model: "", choices: [], chosen: "", warning: nil),
        languageNotice: nil, voiceNotice: nil
    )

    private var store: ModelDetailsStore!

    private init() {
        let relay = ModelDetailsRelay { [weak self] page in Task { @MainActor in self?.show(page) } }
        store = ModelDetailsStore(observer: relay)
    }

    /// `stage`'s card shows `model` of `source`: read what it shows of it.
    /// Again for the model already focused, which reads it again.
    func focus(_ stage: PipelineStage, source: String, model: String) {
        #if DEBUG
        if pretending { return }
        #endif
        store.focus(stage: stage, source: source, model: model)
    }

    /// The language button of the model `stage` shows, once focused.
    func language(_ stage: PipelineStage) -> ModelLanguageRow? {
        page.languages.first { $0.stage == stage }
    }

    /// The language picker's rows for what the search holds: Follow global
    /// (an empty id), Auto-detect when the model offers it, then its other
    /// languages by name.
    func languageRows(_ stage: PipelineStage, _ query: String) -> [Choice] {
        #if DEBUG
        if pretending {
            return pretendRows.filter {
                $0.id.isEmpty || $0.id == "auto" || query.isEmpty
                    || $0.name.localizedCaseInsensitiveContains(query)
            }
        }
        #endif
        return store.languageRows(stage: stage, query: query)
    }

    func send(_ action: ModelDetailsAction) {
        store.send(action: action)
    }

    private func show(_ page: ModelDetailsPage) {
        #if DEBUG
        if pretending { return }
        #endif
        self.page = page
    }

    #if DEBUG
    private var pretending = false
    private var pretendRows: [Choice] = []

    /// Shows `page`, and `languageRows` in the picker, from now on, whatever
    /// the store says: a made-up scene's.
    func pretend(_ page: ModelDetailsPage, languageRows: [Choice] = []) {
        pretending = true
        self.page = page
        pretendRows = languageRows
    }
    #endif
}

/// Hands the store's calls, which come on its own threads, to the model.
final class ModelDetailsRelay: ModelDetailsObserver {
    let onChange: @Sendable (ModelDetailsPage) -> Void

    init(onChange: @escaping @Sendable (ModelDetailsPage) -> Void) {
        self.onChange = onChange
    }

    func changed(page: ModelDetailsPage) {
        onChange(page)
    }
}

/// A model's language: a button labelled with what it resolves to, which
/// opens a search over the languages the model can be set to. A model with
/// one language shows it, with nothing to pick. Nothing is drawn until its
/// language is known.
struct ModelLanguageButton: View {
    let stage: PipelineStage
    let source: String
    let model: String
    @ObservedObject private var details = ModelDetailsModel.shared
    @State private var choosing = Self.startsChoosing

    init(stage: PipelineStage, source: String, model: String) {
        self.stage = stage
        self.source = source
        self.model = model
    }

    /// Whether the picker starts up: only for its shot.
    private static var startsChoosing: Bool {
        #if DEBUG
        return Snapshot.scene == "model-language"
        #else
        return false
        #endif
    }

    var body: some View {
        let row = details.language(stage).flatMap { $0.source == source && $0.model == model ? $0 : nil }
        // A stack, not a group: it is there while empty, so it still focuses
        // the model.
        HStack(spacing: 0) {
            if let row, let label = row.label {
                Button(label) { choosing = true }
                    .disabled(!row.pickable)
            }
        }
        .task(id: [source, model]) { details.focus(stage, source: source, model: model) }
        .sheet(isPresented: $choosing) {
            LanguagePicker(
                title: t("model-language"),
                rows: { details.languageRows(stage, $0) },
                selected: row?.chosen ?? "",
                pick: { details.send(.languagePicked(stage: stage, id: $0)) },
                isPresented: $choosing
            )
        }
    }
}

/// The synthesis model's voice: its own default first, then its voices, a
/// cloned one marked. Under it, when the model can't speak until a voice is
/// chosen, what to do about it.
struct ModelVoicePicker: View {
    let source: String
    let model: String
    @ObservedObject private var details = ModelDetailsModel.shared

    init(source: String, model: String) {
        self.source = source
        self.model = model
    }

    var body: some View {
        let voice = details.page.voice
        let shown = voice.source == source && voice.model == model
        VStack(alignment: .leading, spacing: 6) {
            if shown, !voice.choices.isEmpty {
                Picker(t("voice"), selection: Binding(
                    get: { voice.chosen },
                    set: { details.send(.voicePicked(id: $0)) }
                )) {
                    ForEach(voice.choices, id: \.id) { Text($0.name).tag($0.id) }
                }
            }
            if shown, let warning = voice.warning {
                Label {
                    Text(warning)
                } icon: {
                    IconView(icon: .warning)
                }
                .foregroundStyle(.secondary)
            }
        }
        .task(id: [source, model]) { details.focus(.synthesis, source: source, model: model) }
    }
}

/// What went wrong reading or setting a model's language or voice, for the
/// top of the Models page.
struct ModelDetailsNotices: View {
    @ObservedObject private var details = ModelDetailsModel.shared

    var body: some View {
        if let notice = details.page.languageNotice {
            NoticeBanner(text: notice)
        }
        if let notice = details.page.voiceNotice {
            NoticeBanner(text: notice)
        }
    }
}

#if DEBUG
/// The `model-language` and `model-voice` scenes: each control alone in a
/// form, with made-up languages and voices, for the layout.
struct ModelDetailsScene: View {
    init() {
        Self.pretend()
    }

    var body: some View {
        Form {
            Section {
                if Snapshot.scene == "model-language" {
                    LabeledContent(t("language")) {
                        ModelLanguageButton(stage: .transcription, source: "whisper", model: "large-v3-turbo")
                    }
                } else {
                    ModelVoicePicker(source: "kokoro", model: "kokoro-82m")
                }
            } header: {
                Text(Snapshot.scene == "model-language" ? t("speech-to-text") : t("text-to-speech"))
            }
        }
        .formStyle(.grouped)
        .onAppear { Snapshot.modelDetailsShown() }
    }

    private static func pretend() {
        ModelDetailsModel.shared.pretend(
            ModelDetailsPage(
                languages: [
                    ModelLanguageRow(
                        stage: .transcription, source: "whisper", model: "large-v3-turbo",
                        label: t("language-fr"), pickable: true, chosen: "fr"
                    ),
                ],
                voice: ModelVoiceRow(
                    source: "kokoro", model: "kokoro-82m",
                    choices: [
                        Choice(id: "", name: t("voice-default", ["label": "Heart"])),
                        Choice(id: "af_heart", name: "Heart"),
                        Choice(id: "af_bella", name: "Bella"),
                        Choice(id: "bm_george", name: "George"),
                        Choice(id: "grace", name: t("voice-cloned", ["label": "Grace"])),
                    ],
                    chosen: "grace", warning: nil
                ),
                languageNotice: nil, voiceNotice: nil
            ),
            languageRows: [
                Choice(id: "", name: t("follow-global")),
                Choice(id: "auto", name: t("auto-detect")),
                Choice(id: "en", name: t("language-en")),
                Choice(id: "fr", name: t("language-fr")),
                Choice(id: "de", name: t("language-de")),
                Choice(id: "it", name: t("language-it")),
                Choice(id: "ja", name: t("language-ja")),
                Choice(id: "es", name: t("language-es")),
            ]
        )
    }
}
#endif
