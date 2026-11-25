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
        .onAppear { models.send(.reload) }
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

/// The open step's card: which backend fills the stage, which of its models
/// runs and on what, how it stands, and Load or Unload. It waits while a
/// change runs.
struct StageCardView: View {
    @ObservedObject var models: ModelsModel
    let stage: PipelineStage
    let step: PipelineStep?
    let card: StageCard?

    var body: some View {
        let busy = card?.busy ?? true
        VStack(spacing: 10) {
            PickerRow(label: t("backend")) {
                Picker(t("backend"), selection: Binding(
                    get: { card?.source },
                    set: { models.send(.pickBackend(stage: stage, source: $0)) }
                )) {
                    Text(stage.emptyTitle).tag(String?.none)
                    ForEach(card?.backends ?? [], id: \.id) { backend in
                        Text(backend.name).tag(String?.some(backend.id))
                    }
                }
                .disabled(busy)
            }
            if let card, card.source != nil {
                Divider()
                PickerRow(label: t("model")) {
                    Picker(t("model"), selection: Binding(
                        get: { card.model },
                        set: { model in if let model { models.send(.pickModel(stage: stage, model: model)) } }
                    )) {
                        if card.model == nil {
                            Text("\u{2014}").tag(String?.none)
                        }
                        ForEach(card.models, id: \.self) { Text($0).tag(String?.some($0)) }
                    }
                    .disabled(busy || card.models.isEmpty)
                }
                if !card.devices.isEmpty {
                    Divider()
                    PickerRow(label: t("runs-on")) {
                        Picker(t("runs-on"), selection: Binding(
                            get: { card.device ?? "" },
                            set: { models.send(.pickDevice(stage: stage, device: $0)) }
                        )) {
                            ForEach(card.devices, id: \.id) { Text($0.name).tag($0.id) }
                        }
                        .disabled(busy)
                    }
                }
            }
            Divider()
            HStack {
                Text(t("status"))
                Spacer()
                if busy, card != nil {
                    ProgressView().controlSize(.small)
                    Text(t("loading-progress"))
                        .foregroundStyle(.secondary)
                } else {
                    Circle()
                        .fill((step?.readiness ?? .none).color)
                        .frame(width: 8, height: 8)
                    Text((step?.readiness ?? .none).words)
                        .foregroundStyle(.secondary)
                }
                if let card, card.model != nil {
                    if card.loaded {
                        Button(t("unload")) { models.send(.unload(stage: stage)) }
                            .disabled(busy)
                    } else {
                        Button(t("load-model")) { models.send(.load(stage: stage)) }
                            .buttonStyle(.borderedProminent)
                            .disabled(busy)
                    }
                }
            }
        }
        .padding(12)
        .background(Color(nsColor: .quaternarySystemFill), in: RoundedRectangle(cornerRadius: 8))
    }
}

/// A card's row: its name, and the menu on the right at its own width.
struct PickerRow<Content: View>: View {
    let label: String
    @ViewBuilder let content: Content

    var body: some View {
        HStack {
            Text(label)
            Spacer()
            // A width of its own: sized to fit, a menu takes its first item's,
            // and cuts a longer choice short.
            content
                .labelsHidden()
                .frame(minWidth: 200, alignment: .trailing)
                .fixedSize()
        }
    }
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
