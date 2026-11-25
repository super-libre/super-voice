// SPDX-License-Identifier: GPL-3.0-only
// The Models page: each feature's pipeline as a row of steps, as in the
// settings app, and under the rows the open step's card. Each step says which
// backend fills its stage and how its model stands. The card picks the
// backend, its model and where it runs, and loads or unloads it.

import SuperVoiceCore
import SwiftUI

@MainActor
final class ModelsModel: ObservableObject {
    static let shared = ModelsModel()

    @Published private(set) var page = ModelsPage(cards: [], notice: nil)

    private var store: ModelsStore!

    private init() {
        let relay = ModelsRelay { [weak self] page in Task { @MainActor in self?.show(page) } }
        store = ModelsStore(observer: relay)
    }

    private func show(_ page: ModelsPage) {
        #if DEBUG
        if pretending { return }
        #endif
        self.page = page
    }

    #if DEBUG
    private var pretending = false

    /// Shows `page` from now on, whatever the store says: a made-up scene's.
    func pretend(_ page: ModelsPage) {
        pretending = true
        self.page = page
    }
    #endif

    func send(_ action: ModelsAction) {
        store.send(action: action)
    }
}

/// Hands the store's calls, which come on its own threads, to the model.
final class ModelsRelay: ModelsObserver {
    let onChange: @Sendable (ModelsPage) -> Void

    init(onChange: @escaping @Sendable (ModelsPage) -> Void) {
        self.onChange = onChange
    }

    func changed(page: ModelsPage) {
        onChange(page)
    }
}

struct ModelsView: View {
    @ObservedObject var header = HeaderModel.shared
    @ObservedObject var models = ModelsModel.shared
    @State private var open: PipelineStage = .transcription

    var body: some View {
        let steps = header.header.pipeline
        VStack(spacing: 0) {
        if let notice = models.page.notice {
            NoticeBanner(text: notice)
        }
        ModelDetailsNotices()
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                FeatureRow(
                    icon: .microphone,
                    name: t("speech-to-text"),
                    items: [
                        .end(t("microphone"), t("your-voice")),
                        .step(.transcription),
                        .step(.postProcessing),
                        .end(t("typed"), t("into-your-app")),
                    ],
                    steps: steps,
                    open: $open
                )
                FeatureRow(
                    icon: .speakerHigh,
                    name: t("text-to-speech"),
                    items: [
                        .end(t("text"), t("from-your-app")),
                        .step(.synthesis),
                        .end(t("speakers"), t("read-aloud")),
                    ],
                    steps: steps,
                    open: $open
                )
                Divider()
                Text(open.name)
                    .font(.title2.bold())
                StageCardView(
                    models: models,
                    stage: open,
                    step: steps.first { $0.stage == open },
                    card: models.page.cards.first { $0.stage == open }
                )
            }
            // The other pages' grouped cards' inset.
            .padding(.horizontal, 28)
            .padding(.vertical)
        }
        }
        .navigationTitle(t("models"))
        .onAppear {
            models.send(.reload)
            // The cards' update chips are the Library's offers.
            LibraryModel.shared.send(.reload)
        }
        #if DEBUG
        .onAppear { Snapshot.modelsShown(header.header) }
        .onChange(of: steps.count) { _, _ in Snapshot.modelsShown(header.header) }
        #endif
    }
}

/// One item of a feature's pipeline.
enum PipelineItem {
    /// Where the feature starts or stops: a name and a line about it.
    case end(String, String)
    /// A stage the user sets.
    case step(PipelineStage)
}

/// One feature: its name over its items, each joined to the next by a caret
/// pointing the way the feature runs.
struct FeatureRow: View {
    let icon: Icon
    let name: String
    let items: [PipelineItem]
    let steps: [PipelineStep]
    @Binding var open: PipelineStage

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                IconView(icon: icon, size: 18)
                Text(name)
                    .font(.title3.bold())
            }
            HStack(spacing: 6) {
                ForEach(Array(items.enumerated()), id: \.offset) { index, item in
                    if index > 0 {
                        IconView(icon: .caretRight, size: 14, color: .tertiaryLabelColor)
                    }
                    switch item {
                    case let .end(name, caption):
                        // Nothing to set where a feature starts or stops, so
                        // no surface: a fixed width keeps both rows' steps
                        // starting at the same place.
                        VStack(alignment: .leading, spacing: 2) {
                            Text(name).fontWeight(.semibold)
                            Text(caption)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        .frame(width: 112, alignment: .leading)
                    case let .step(stage):
                        StepButton(stage: stage, step: steps.first { $0.stage == stage }, open: $open)
                    }
                }
            }
        }
    }
}

/// A stage: its name and dot, the backend in it and how its model stands, and
/// "Idle" in its corner while its model is out of memory until its next use.
/// Pressing it opens its card; the open one wears the accent border.
struct StepButton: View {
    let stage: PipelineStage
    let step: PipelineStep?
    @Binding var open: PipelineStage

    var body: some View {
        let isOpen = open == stage
        Button {
            open = stage
        } label: {
            VStack(alignment: .leading, spacing: 2) {
                HStack {
                    Text(stage.name)
                        .font(.caption)
                        .foregroundStyle(isOpen ? Color.accentColor : .secondary)
                    Spacer()
                    Circle()
                        .fill((step?.readiness ?? .none).color)
                        .frame(width: 8, height: 8)
                }
                Text(step?.title(stage) ?? stage.emptyTitle)
                    .fontWeight(.semibold)
                    .lineLimit(1)
                HStack {
                    Text(step?.detail(stage) ?? stage.emptyDetail)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                    if case .idle = step?.model {
                        Spacer()
                        Text(t("idle"))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .padding(.vertical, 10)
            .padding(.horizontal, 12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color(nsColor: .quaternarySystemFill), in: RoundedRectangle(cornerRadius: 8))
            .overlay(
                RoundedRectangle(cornerRadius: 8)
                    .strokeBorder(isOpen ? Color.accentColor : Color(nsColor: .separatorColor), lineWidth: 1)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

/// The open step's card, as in the settings app: the backend filling the
/// stage, with its chips, repository, Configure and ✕; then the model staged
/// and where it runs, with Load; or the model running, with Unload. Under them
/// the download or the load as it goes, or why it failed. While a change runs,
/// the card waits.
struct StageCardView: View {
    @ObservedObject var models: ModelsModel
    let stage: PipelineStage
    let step: PipelineStep?
    let card: StageCard?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let card, let backend = card.backend {
                CardHeader(models: models, stage: stage, backend: backend, busy: card.busy)
                Divider()
                if card.notes.unmet.isEmpty {
                    CardModelRows(models: models, stage: stage, card: card)
                } else {
                    ForEach(card.notes.unmet, id: \.self) { line in
                        WarningLine(text: line)
                    }
                }
                CardProgressView(models: models, stage: stage, progress: card.progress)
                if case .running(_, true) = card.model {
                    Text(t("idle"))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            } else {
                EmptyStageView(models: models, stage: stage, card: card)
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color(nsColor: .quaternarySystemFill), in: RoundedRectangle(cornerRadius: 8))
    }
}

/// The backend's name and version, its chips and description, and its
/// repository, Configure and ✕ on the right.
struct CardHeader: View {
    @ObservedObject var models: ModelsModel
    let stage: PipelineStage
    let backend: CardBackend
    let busy: Bool

    var body: some View {
        HStack(alignment: .top) {
            VStack(alignment: .leading, spacing: 4) {
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    Text(backend.name).font(.headline)
                    if !backend.version.isEmpty {
                        Text(backend.version)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
                HStack(spacing: 4) {
                    BackendChipsView(backend: backend)
                    UpdateChip(source: backend.source)
                }
                if !backend.description.isEmpty {
                    Text(backend.description)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            Spacer()
            if let url = URL(string: backend.repository) {
                Link(destination: url) {
                    IconView(icon: .gitBranch)
                }
                .help(t("open-repository"))
            }
            ConfigureButton(source: backend.source)
            Button {
                models.send(.pickBackend(stage: stage, source: nil))
            } label: {
                IconView(icon: .x)
            }
            .buttonStyle(.borderless)
            .help(t("deselect-this-backend"))
            .disabled(busy)
        }
    }
}

/// The update waiting for a card's backend, as the Library offers it: the
/// action as well as the sign, so a version waiting shows on the page the user
/// sits on. While it installs, a spinner.
struct UpdateChip: View {
    @ObservedObject var library = LibraryModel.shared
    let source: String

    var body: some View {
        if let backend = library.page.installed.first(where: { $0.source == source }) {
            if case .running = backend.update {
                ProgressView().controlSize(.mini)
            } else if let version = backend.updateTo {
                Button(t("update-to-version", ["version": version])) {
                    library.send(.update(source: source))
                }
                .controlSize(.small)
            }
        }
    }
}

/// What a backend runs its models on, and how many it offers the stage.
struct BackendChipsView: View {
    let backend: CardBackend

    var body: some View {
        HStack(spacing: 4) {
            if backend.chips.gpu {
                Chip(icon: .graphicsCard, word: "GPU").help(t("accelerated-on-gpu"))
            }
            if backend.chips.cpu {
                Chip(icon: .cpu, word: "CPU").help(t("runs-on-the-cpu"))
            }
            if backend.chips.online {
                Chip(icon: .cloud, word: t("cloud")).help(t("runs-in-the-cloud"))
            }
            Chip(icon: nil, word: t("models-count", ["model_count": String(backend.models)]))
        }
    }
}

/// A small capsule: an icon and a word.
struct Chip: View {
    let icon: Icon?
    let word: String

    var body: some View {
        HStack(spacing: 4) {
            if let icon {
                IconView(icon: icon, size: 12, color: .secondaryLabelColor)
            }
            Text(word)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding(.vertical, 2)
        .padding(.horizontal, 7)
        .background(Capsule().fill(Color(nsColor: .quaternarySystemFill)))
    }
}

/// The model rows: the model, where it runs, and the buttons. A running model
/// shows its name with Unload (and Reload on synthesis); a staged one
/// a menu of the backend's models with Load.
struct CardModelRows: View {
    @ObservedObject var models: ModelsModel
    let stage: PipelineStage
    let card: StageCard

    var body: some View {
        let busy = card.busy
        VStack(alignment: .leading, spacing: 10) {
            switch card.model {
            case let .running(model, _):
                LabeledRow(label: t("model")) {
                    Text(model).foregroundStyle(Color.accentColor)
                }
            case let .staging(model):
                LabeledRow(label: t("model"), note: card.notes.memory) {
                    Picker(t("model"), selection: Binding(
                        get: { model },
                        set: { picked in if let picked { models.send(.pickModel(stage: stage, model: picked)) } }
                    )) {
                        if model == nil {
                            Text(t("select-model")).tag(String?.none)
                        }
                        ForEach(card.models, id: \.self) { Text($0).tag(String?.some($0)) }
                    }
                    .disabled(busy || card.models.isEmpty)
                }
            case .empty:
                EmptyView()
            }
            if !card.devices.isEmpty {
                LabeledRow(label: t("runs-on"), note: card.notes.fallback) {
                    Picker(t("runs-on"), selection: Binding(
                        get: { card.device ?? "" },
                        set: { models.send(.pickDevice(stage: stage, device: $0)) }
                    )) {
                        if card.device == nil {
                            Text(t("select")).tag("")
                        }
                        ForEach(card.devices, id: \.id) { Text($0.name).tag($0.id) }
                    }
                    .disabled(busy)
                }
            }
            ModelDetailsSlot(stage: stage, card: card)
            if let warning = card.notes.warning {
                WarningLine(text: warning)
            }
            HStack {
                Spacer()
                switch card.model {
                case .running:
                    if card.reloadable {
                        Button(t("reload-this-model")) { models.send(.reloadModel(stage: stage)) }
                            .disabled(busy)
                    }
                    Button {
                        models.send(.unload(stage: stage))
                    } label: {
                        Label { Text(t("unload")) } icon: { IconView(icon: .stop, size: 12) }
                    }
                    .disabled(busy)
                case .staging:
                    Button {
                        models.send(.load(stage: stage))
                    } label: {
                        Label { Text(t("load-model")) } icon: { IconView(icon: .play, size: 12, color: .white) }
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(!card.canLoad)
                case .empty:
                    EmptyView()
                }
            }
        }
    }
}

/// The shown model's language, when it has more than one, and for synthesis
/// its voice.
struct ModelDetailsSlot: View {
    let stage: PipelineStage
    let card: StageCard
    @ObservedObject private var details = ModelDetailsModel.shared

    var body: some View {
        if let source = card.source, let model = card.model.name {
            let known = details.language(stage).map {
                $0.source == source && $0.model == model && $0.label != nil
            } ?? false
            if known {
                LabeledRow(label: t("language")) {
                    ModelLanguageButton(stage: stage, source: source, model: model)
                }
            } else {
                // Draws nothing until the language is read, which it asks for.
                ModelLanguageButton(stage: stage, source: source, model: model)
            }
            if stage == .synthesis {
                HStack(alignment: .firstTextBaseline) {
                    Text(t("voice"))
                    Spacer()
                    ModelVoicePicker(source: source, model: model)
                        .labelsHidden()
                        .frame(maxWidth: 360, alignment: .trailing)
                }
            }
        }
    }
}

extension CardModel {
    /// The model a card shows, running or staged.
    var name: String? {
        switch self {
        case let .running(model, _): model
        case let .staging(model): model
        case .empty: nil
        }
    }
}

/// A card's row: its name, with a line under it when it has one, and the
/// control on the right.
struct LabeledRow<Content: View>: View {
    let label: String
    var note: String? = nil
    @ViewBuilder let content: Content

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            VStack(alignment: .leading, spacing: 2) {
                Text(label)
                if let note {
                    Text(note)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            Spacer()
            content
                .labelsHidden()
                .frame(minWidth: 200, alignment: .trailing)
                .fixedSize()
        }
    }
}

/// A warning glyph and a sentence.
struct WarningLine: View {
    let text: String

    var body: some View {
        Label {
            Text(text).fixedSize(horizontal: false, vertical: true)
        } icon: {
            IconView(icon: .warning, color: .systemOrange)
        }
    }
}

/// The stage's download, with Cancel; its load, with the backend's step; or
/// why it failed.
struct CardProgressView: View {
    @ObservedObject var models: ModelsModel
    let stage: PipelineStage
    let progress: CardProgress

    var body: some View {
        switch progress {
        case .none:
            EmptyView()
        case let .download(line, fraction, bytes):
            VStack(alignment: .leading, spacing: 6) {
                Text(line)
                ProgressView(value: Double(max(fraction, 0.02)))
                HStack {
                    Text(bytes)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Spacer()
                    Button(t("cancel"), role: .destructive) {
                        models.send(.cancelDownload(stage: stage))
                    }
                }
            }
        case let .load(title, step, fraction):
            VStack(alignment: .leading, spacing: 6) {
                Text(title)
                HStack {
                    Text(step)
                    Spacer()
                    if let fraction {
                        Text("\(Int((fraction * 100).rounded()))%")
                    }
                }
                .font(.caption)
                .foregroundStyle(.secondary)
                if let fraction {
                    ProgressView(value: Double(max(fraction, 0.02)))
                }
            }
        case let .working(line):
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text(line).foregroundStyle(.secondary)
            }
        case let .failed(message):
            WarningLine(text: message)
        }
    }
}

/// A stage nothing fills: what it is for, and a menu of the backends that can
/// fill it. When none can, the Library is where to install one.
struct EmptyStageView: View {
    @ObservedObject var models: ModelsModel
    let stage: PipelineStage
    let card: StageCard?

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(t("no-backend-selected")).font(.headline)
            Text(stage.unselectedLine)
                .foregroundStyle(.secondary)
            if let card {
                if card.backends.isEmpty {
                    Text(stage.noBackendLine)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                    Button(t("open-library")) {
                        NotificationCenter.default.post(name: .openLibrary, object: stage)
                    }
                    .buttonStyle(.borderedProminent)
                } else {
                    Menu(stage.selectLine) {
                        ForEach(card.backends, id: \.source) { backend in
                            Button(backend.name) {
                                models.send(.pickBackend(stage: stage, source: backend.source))
                            }
                        }
                    }
                    .fixedSize()
                    .disabled(card.busy)
                }
            } else {
                ProgressView().controlSize(.small)
            }
        }
    }
}

extension Notification.Name {
    /// Open the Library, to install a backend for the stage in `object`.
    static let openLibrary = Notification.Name("SuperVoiceOpenLibrary")
}

extension PipelineStage {
    /// The name a stage goes by on its step and above its card.
    var name: String {
        switch self {
        case .transcription: t("transcription")
        case .postProcessing: t("post-processing")
        case .synthesis: t("synthesis")
        }
    }

    /// What a step says with no backend. Post-processing is optional: without
    /// one, transcripts are typed as they are.
    var emptyTitle: String {
        self == .postProcessing ? t("off") : t("none")
    }

    var emptyDetail: String {
        self == .postProcessing ? t("optional") : t("select-a-backend")
    }

    /// What an empty card says the stage is for.
    var unselectedLine: String {
        switch self {
        case .transcription: t("select-a-backend-to-start-transcribing")
        case .postProcessing: t("optional-clean-up-text-before-it-is")
        case .synthesis: t("select-a-backend-to-start-speaking")
        }
    }

    /// The menu of backends that can fill the stage.
    var selectLine: String {
        switch self {
        case .transcription: t("select-transcription-backend")
        case .postProcessing: t("select-post-processor-backend")
        case .synthesis: t("select-synthesis-backend")
        }
    }

    /// What an empty card says when nothing installed can fill the stage.
    var noBackendLine: String {
        switch self {
        case .transcription: t("no-installed-backend-provides-a-transcription-model")
        case .postProcessing: t("no-installed-backend-provides-a-post-processor")
        case .synthesis: t("no-installed-backend-provides-text-to-speech")
        }
    }
}

extension PipelineStep {
    func title(_ stage: PipelineStage) -> String {
        backend ?? stage.emptyTitle
    }

    /// The model and the framework it runs on, or why nothing runs.
    func detail(_ stage: PipelineStage) -> String {
        guard backend != nil else { return stage.emptyDetail }
        switch model {
        case let .running(name, device): return device.map { "\(name) · \($0)" } ?? name
        case let .idle(name): return name
        case .notLoaded: return t("not-loaded")
        }
    }
}
