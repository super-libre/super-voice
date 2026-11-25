// SPDX-License-Identifier: GPL-3.0-only
// The Clips tab: a card to type the next clip into and make it, in the voice
// text to speech uses; then the clips, newest first, each to play, export or
// delete.

import SuperVoiceCore
import SwiftUI

struct ClipsView: View {
    @ObservedObject var model: ClipsModel
    @ObservedObject var header = HeaderModel.shared

    var body: some View {
        VStack(spacing: 0) {
            if let notice = model.page.notice {
                NoticeBanner(text: notice.words)
            }
            Form {
                Section {
                    if model.page.making {
                        MakingCard(model: model)
                    } else {
                        NewClipCard(model: model, readiness: header.header.textToSpeech)
                    }
                } header: {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(t("type-something-and-keep-it-as-speech"))
                            .font(.body)
                            .foregroundStyle(.primary)
                        Text(t("clips-are-saved-on-this-computer"))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    .textCase(nil)
                    .padding(.bottom, 6)
                }

                Section {
                    if model.page.loaded && model.page.clips.isEmpty {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(t("no-clips-yet"))
                            Text(t("each-clip-you-make-shows-here"))
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                    ForEach(model.page.clips, id: \.id) { clip in
                        ClipRow(model: model, clip: clip)
                    }
                } header: {
                    HStack {
                        Text(t("clips"))
                        Spacer()
                        Text(t("clips-count", ["count": "\(model.page.clips.count)"]))
                            .foregroundStyle(.secondary)
                    }
                    .textCase(nil)
                }
            }
            .formStyle(.grouped)
        }
        .onAppear { model.send(.reload) }
    }
}

/// The card the next clip is typed into: its text and Make clip. The voice is
/// chosen on the Models page, with the model.
struct NewClipCard: View {
    @ObservedObject var model: ClipsModel
    let readiness: Readiness

    /// Roughly how long speech takes: about 150 words a minute.
    private static let secondsPerWord = 0.4

    var body: some View {
        let unready = notReady(feature: .textToSpeech, readiness: readiness)
        let words = model.page.draftWords
        VStack(alignment: .leading, spacing: 8) {
            Text(t("new-clip"))
            TextEditor(text: $model.draft)
                .font(.body)
                .frame(minHeight: 96)
                .scrollContentBackground(.hidden)
                .padding(6)
                .background(Color(nsColor: .textBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
                .overlay(alignment: .topLeading) {
                    if model.draft.isEmpty {
                        Text(t("what-the-clip-should-say"))
                            .foregroundStyle(.tertiary)
                            .padding(.horizontal, 11)
                            .padding(.vertical, 6)
                            .allowsHitTesting(false)
                    }
                }
                .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Color(nsColor: .separatorColor)))
                .onChange(of: model.draft) { _, text in model.draftChanged(text) }
            HStack {
                Text(estimate(unready: unready, words: words))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Spacer()
                Button(t("make-clip")) { model.send(.make) }
                    .buttonStyle(.borderedProminent)
                    .disabled(words == 0 || unready != nil)
            }
        }
        .padding(.vertical, 4)
    }

    /// Why Make clip is off, or what the draft will come to.
    private func estimate(unready: NotReady?, words: UInt32) -> String {
        if let unready {
            return unready.words
        }
        if words == 0 {
            return t("type-what-the-clip-should-say")
        }
        let seconds = Int((Double(words) * Self.secondsPerWord).rounded(.up))
        return "\(wordCount(words)) · " + t("about-seconds", ["seconds": "\(seconds)"])
    }
}

/// The card while a clip is made: how long it has taken, Cancel, and a bar
/// that moves, since the daemon can't say how far it has got.
struct MakingCard: View {
    @ObservedObject var model: ClipsModel

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(t("making-the-clip"))
                    TimelineView(.periodic(from: .now, by: 1)) { context in
                        Text(clock(context.date.timeIntervalSince(model.makingSince ?? context.date)))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .monospacedDigit()
                    }
                }
                Spacer()
                Button(t("cancel")) { model.send(.cancel) }
            }
            ProgressView()
                .progressViewStyle(.linear)
            Text(firstLine(model.draft))
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
        .padding(.vertical, 4)
    }
}

/// One clip: what it says, when and in what voice, and its buttons. Delete
/// asks again in the row, as in the settings app.
struct ClipRow: View {
    @ObservedObject var model: ClipsModel
    let clip: ClipSummary

    var body: some View {
        HStack(spacing: 8) {
            VStack(alignment: .leading, spacing: 2) {
                Text(firstLine(clip.text))
                    .lineLimit(1)
                Text(about)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
                if model.page.exportedId == clip.id, let path = model.page.exportedPath {
                    Text(t("exported-to", ["path": path]))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
            }
            Spacer()
            if model.page.confirmingDelete == clip.id {
                Text(t("delete-this-clip"))
                Button(t("cancel")) { model.send(.deleteCancelled) }
                Button(t("delete")) { model.send(.deleteConfirmed(id: clip.id)) }
                    .buttonStyle(.borderedProminent)
                    .tint(.red)
            } else {
                if model.page.playing == clip.id {
                    Button(t("stop")) { model.send(.stop) }
                } else {
                    Button(t("play")) { model.send(.play(id: clip.id)) }
                }
                Button(t("export")) { model.send(.export(id: clip.id)) }
                Button(t("delete")) { model.send(.deleteRequested(id: clip.id)) }
                    .buttonStyle(.borderedProminent)
                    .tint(.red)
            }
        }
        .padding(.vertical, 2)
    }

    /// When and in what voice, and how long; or how far it has played.
    private var about: String {
        if model.page.playing == clip.id {
            guard let position = model.page.playingPosition, let length = model.page.playingLength else {
                return t("starting")
            }
            return t("playing-position", [
                "position": clock(TimeInterval(position)),
                "length": clock(TimeInterval(length)),
            ])
        }
        var parts = [when(clip.created)]
        if let voice = clip.voice {
            parts.append(voice)
        }
        parts.append(clock(TimeInterval(clip.seconds)))
        return parts.joined(separator: " · ")
    }
}

extension ClipsNotice {
    /// What the page says about it.
    var words: String {
        switch self {
        case let .couldNotRead(message): t("clips-could-not-be-read", ["error": message])
        case let .couldNotMake(message): t("the-clip-could-not-be-made", ["error": message])
        case let .couldNotPlay(message): t("the-clip-could-not-play", ["error": message])
        case let .couldNotExport(message): t("the-clip-could-not-be-exported", ["error": message])
        case let .couldNotDelete(message): t("the-clip-could-not-be-deleted", ["error": message])
        }
    }
}

extension NotReady {
    /// What a button that is off says about it.
    var words: String {
        switch self {
        case .noVoice: t("choose-a-voice-on-the-models-page")
        case .textToSpeech: t("text-to-speech-is-not-ready")
        case .speechToText: t("speech-to-text-is-not-ready")
        }
    }
}

/// `seconds` as minutes and seconds: "0:42", "10:00".
func clock(_ seconds: TimeInterval) -> String {
    let whole = max(0, Int(seconds.rounded(.down)))
    return String(format: "%d:%02d", whole / 60, whole % 60)
}
