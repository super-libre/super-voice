// SPDX-License-Identifier: GPL-3.0-only
// The pills at the top right, as in the settings app: the GPU and its memory,
// then a dot for how ready each feature's model is.

import AppKit
import SuperVoiceCore
import SwiftUI

@MainActor
final class HeaderModel: ObservableObject {
    static let shared = HeaderModel()

    @Published private(set) var header = HeaderStatus(
        connected: false, gpus: [], speechToText: .none, textToSpeech: .none, pipeline: []
    )

    private var store: StatusStore!

    private init() {
        let relay = StatusRelay { [weak self] header in
            Task { @MainActor in self?.show(header) }
        }
        store = StatusStore(observer: relay)
    }

    private func show(_ header: HeaderStatus) {
        #if DEBUG
        if pretending { return }
        #endif
        self.header = header
    }

    #if DEBUG
    private var pretending = false

    /// Shows `header` from now on, whatever the daemon says: a made-up
    /// scene's.
    func pretend(_ header: HeaderStatus) {
        pretending = true
        self.header = header
    }
    #endif
}

/// Hands the store's calls, which come on its own threads, to the model.
final class StatusRelay: StatusObserver {
    let onChange: @Sendable (HeaderStatus) -> Void

    init(onChange: @escaping @Sendable (HeaderStatus) -> Void) {
        self.onChange = onChange
    }

    func changed(header: HeaderStatus) {
        onChange(header)
    }
}

/// The pills. Nothing until the daemon answers, as in the settings app.
struct HeaderPills: View {
    @ObservedObject var model: HeaderModel

    var body: some View {
        if model.header.connected {
            HStack(spacing: 8) {
                if let gpu = model.header.gpus.first {
                    GpuPill(gpu: gpu, all: model.header.gpus)
                }
                FeaturePill(icon: .microphone, feature: t("speech-to-text"), readiness: model.header.speechToText)
                FeaturePill(icon: .speakerHigh, feature: t("text-to-speech"), readiness: model.header.textToSpeech)
            }
        }
    }
}

/// A neutral rounded surface, as the settings app's header pills: a readout,
/// not a button.
struct Pill<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        content
            .padding(.horizontal, 10)
            .padding(.vertical, 5)
            // No border, unlike the settings app's: a stroked capsule in the
            // toolbar draws stray marks at its ends in the app's screenshots.
            .background(Capsule().fill(Color(nsColor: .quaternaryLabelColor).opacity(0.5)))
    }
}

/// A feature's glyph and a dot in its readiness's colour. The tooltip says
/// what the colour means, so the dot is not colour alone.
struct FeaturePill: View {
    let icon: Icon
    let feature: String
    let readiness: Readiness

    var body: some View {
        Pill {
            HStack(spacing: 6) {
                IconView(icon: icon, size: 14)
                Circle()
                    .fill(readiness.color)
                    .frame(width: 10, height: 10)
            }
        }
        .help("\(feature): \(readiness.words)")
    }
}

extension Readiness {
    var color: Color {
        switch self {
        case .none: Color(nsColor: .systemGray)
        case .blocked, .failed, .noVoice: Color(nsColor: .systemRed)
        case .notLoaded: Color(nsColor: .systemYellow)
        case .ready, .idle: Color(nsColor: .systemGreen)
        }
    }

    /// What the dot means, after the feature's name.
    var words: String {
        switch self {
        case .none: t("no-backend-selected")
        case .blocked: t("backend-setup-incomplete")
        case .notLoaded: t("no-model-loaded")
        case .ready: t("model-loaded-and-ready")
        case .idle: t("model-ready")
        case .failed: t("the-selected-model-isn-t-loaded")
        case .noVoice: t("no-cloned-voice-chosen")
        }
    }
}

/// The primary GPU: its name, memory in use, and a meter that turns yellow
/// then red as it fills. The tooltip lists every GPU.
struct GpuPill: View {
    let gpu: GpuReadout
    let all: [GpuReadout]

    var body: some View {
        Pill {
            HStack(spacing: 6) {
                IconView(icon: .graphicsCard, size: 14, color: .secondaryLabelColor)
                Text(label)
                    .font(.system(size: 12))
                    .monospacedDigit()
                if let fraction = gpu.fraction, let level = gpu.level {
                    Meter(fraction: fraction, level: level)
                }
            }
        }
        .help(all.map(detail).joined(separator: "\n\n"))
    }

    private var label: String {
        if let used = gpu.usedBytes {
            return "\(gpu.name) · " + t("memory-pair", ["value0": gib(used), "value1": gib(gpu.totalBytes)])
        }
        return "\(gpu.name) · " + size(gpu.totalBytes)
    }

    private func detail(_ gpu: GpuReadout) -> String {
        guard let used = gpu.usedBytes else {
            return t("gpu-memory-total", ["value0": gpu.fullName, "value1": size(gpu.totalBytes)])
        }
        let free = gpu.totalBytes > used ? gpu.totalBytes - used : 0
        return t("gpu-memory-summary", [
            "value0": gpu.fullName,
            "value1": size(used),
            "value2": size(free),
            "value3": size(gpu.totalBytes),
        ])
    }

    /// "24.0 GiB".
    private func size(_ bytes: UInt64) -> String {
        t("memory-size", ["gib": gib(bytes)])
    }
}

/// A byte count in GiB, to one decimal.
func gib(_ bytes: UInt64) -> String {
    String(format: "%.1f", Double(bytes) / 1_073_741_824)
}

/// The GPU's memory meter: a neutral track and a fill in the level's colour.
struct Meter: View {
    let fraction: Float
    let level: MeterLevel

    var body: some View {
        ZStack(alignment: .leading) {
            Capsule().fill(Color(nsColor: .systemGray).opacity(0.25))
            Capsule()
                .fill(color)
                .frame(width: max(2, 72 * CGFloat(fraction)))
        }
        .frame(width: 72, height: 6)
    }

    private var color: Color {
        switch level {
        case .ok: Color(nsColor: .systemGreen)
        case .high: Color(nsColor: .systemYellow)
        case .full: Color(nsColor: .systemRed)
        }
    }
}
