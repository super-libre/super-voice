// SPDX-License-Identifier: GPL-3.0-only
// The Models page: each feature's pipeline as a row of steps, as in the
// settings app, and under the rows the open step's card. Each step says which
// backend fills its stage and how its model stands. The card shows what runs;
// choosing a backend or a model isn't in this app yet.

import SuperVoiceCore
import SwiftUI

struct ModelsView: View {
    @ObservedObject var header = HeaderModel.shared
    @State private var open: PipelineStage = .transcription

    var body: some View {
        let steps = header.header.pipeline
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
                StageCard(stage: open, step: steps.first { $0.stage == open })
                Text(t("choosing-models-not-in-this-app-yet"))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            // The other pages' grouped cards' inset.
            .padding(.horizontal, 28)
            .padding(.vertical)
        }
        .navigationTitle(t("models"))
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

/// The open step's card: what fills the stage and how it stands.
struct StageCard: View {
    let stage: PipelineStage
    let step: PipelineStep?

    var body: some View {
        VStack(spacing: 10) {
            CardRow(label: t("backend"), value: step?.backend ?? stage.emptyTitle)
            if step?.backend != nil, let step {
                Divider()
                switch step.model {
                case let .running(name, device):
                    CardRow(label: t("model"), value: name)
                    if let device {
                        Divider()
                        CardRow(label: t("runs-on"), value: device)
                    }
                case let .idle(name):
                    CardRow(label: t("model"), value: name)
                case .notLoaded:
                    CardRow(label: t("model"), value: t("not-loaded"))
                }
            }
            Divider()
            HStack {
                Text(t("status"))
                Spacer()
                Circle()
                    .fill((step?.readiness ?? .none).color)
                    .frame(width: 8, height: 8)
                Text((step?.readiness ?? .none).words)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(12)
        .background(Color(nsColor: .quaternarySystemFill), in: RoundedRectangle(cornerRadius: 8))
    }
}

struct CardRow: View {
    let label: String
    let value: String

    var body: some View {
        HStack {
            Text(label)
            Spacer()
            Text(value)
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .truncationMode(.middle)
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
