// SPDX-License-Identifier: GPL-3.0-only
// The Notes tab: the notes the app keeps, newest first, and the one open.

import SuperVoiceCore
import SwiftUI

struct NotesView: View {
    @ObservedObject var model: NotesModel

    var body: some View {
        VStack(spacing: 0) {
            if let notice = model.page.notice {
                NoticeBanner(text: notice.words)
            }
            if let open = model.page.open {
                OpenNoteView(model: model, open: open)
            } else {
                NotesListView(model: model)
            }
        }
        .onAppear { model.send(.reload) }
        .onDisappear { model.send(.saveNow) }
    }
}

/// The list, with the card a new note is recorded from: grouped boxes, as
/// System Settings draws them and as the settings app's cards are.
struct NotesListView: View {
    @ObservedObject var model: NotesModel

    var body: some View {
        Form {
            Section {
                if let take = model.page.take {
                    TakeRows(model: model, take: take)
                } else {
                    RecordRow(model: model)
                }
            } header: {
                VStack(alignment: .leading, spacing: 4) {
                    Text(t("speak-a-note-and-it-is-kept-here"))
                        .font(.body)
                        .foregroundStyle(.primary)
                    Text(t("notes-are-saved-on-this-computer"))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .textCase(nil)
                .padding(.bottom, 6)
            }

            Section {
                if model.page.loaded && model.page.notes.isEmpty {
                    Text(t("no-notes-yet"))
                        .foregroundStyle(.secondary)
                }
                ForEach(model.page.notes, id: \.id) { note in
                    Button {
                        model.send(.open(id: note.id))
                    } label: {
                        NoteRow(note: note)
                    }
                    .buttonStyle(.plain)
                }
            } header: {
                HStack {
                    Text(t("notes"))
                    Spacer()
                    Text(t("notes-count", ["count": "\(model.page.notes.count)"]))
                        .foregroundStyle(.secondary)
                }
                .textCase(nil)
            }
        }
        .formStyle(.grouped)
    }
}

/// The New note row: Record, or why it is off.
struct RecordRow: View {
    @ObservedObject var model: NotesModel
    @ObservedObject var header = HeaderModel.shared
    @ObservedObject var microphone = MicrophoneModel.shared

    var body: some View {
        // Something else holds the microphone (the Behavior tab's test, or
        // dictation): the daemon would refuse a second take.
        let busy = microphone.microphone.recording
        // A take with no model ready to transcribe it would record and then
        // fail, so Record stays off until speech to text is ready.
        let unready = notReady(feature: .speechToText, readiness: header.header.speechToText)
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(t("new-note"))
                Text(busy ? t("another-recording-is-running") : unready?.words ?? t("press-record-and-speak"))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Button(t("record")) { model.send(.record) }
                .buttonStyle(.borderedProminent)
                .disabled(busy || unready != nil)
        }
    }
}

/// A take recording: which note it goes to, how long it has run, whether it
/// hears speech, Stop, the level, and the words so far. As in the settings
/// app, and laid out as its recording test is.
struct TakeRows: View {
    @ObservedObject var model: NotesModel
    @ObservedObject var microphone = MicrophoneModel.shared
    let take: TakePage

    var body: some View {
        let heard = microphone.microphone
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(label)
                TimelineView(.periodic(from: .now, by: 1)) { context in
                    Text(t("recording-elapsed", [
                        "elapsed": clock(max(0, context.date.timeIntervalSince1970 - take.started)),
                        "limit": clock(TimeInterval(take.limitSeconds)),
                    ]))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
                }
            }
            Spacer()
            Text(heard.speech ? t("speech") : t("silence"))
            Button(take.stopping ? t("stopping") : t("stop"), role: .destructive) { model.send(.stop) }
                .buttonStyle(.borderedProminent)
                .tint(.red)
                .disabled(take.stopping)
        }
        // Rows of their own rather than `LabeledContent`, which lays out as a
        // form's row only inside a form: the open note shows these in a card.
        HStack {
            Text(t("audio-level"))
            Spacer()
            // A floor under any sound at all, so quiet speech still moves the
            // meter.
            ProgressView(value: Double(heard.level > 0 ? max(heard.level, 0.1) : 0))
                .progressViewStyle(.linear)
                .frame(width: 240)
        }
        // The newest words on one line, their end showing, so the rows don't
        // move as they come; what they span goes under the row's name.
        HStack(alignment: .firstTextBaseline) {
            VStack(alignment: .leading, spacing: 2) {
                Text(t("live"))
                if let spans = take.previewSpans {
                    Text(spans.words)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            Spacer(minLength: 24)
            Text(take.preview.isEmpty ? "\u{2014}" : take.preview)
                .lineLimit(1)
                .truncationMode(.head)
                .foregroundStyle(take.preview.isEmpty ? .secondary : .primary)
        }
    }

    /// Which note the words go to. Anywhere but on the note itself, say which.
    private var label: String {
        switch take.into {
        case .newNote: t("new-note")
        case .openNote: t("dictate-more")
        case let .otherNote(created):
            created.map { t("adding-to-note", ["when": when($0)]) } ?? t("dictate-more")
        }
    }
}

extension PreviewSpans {
    /// What the Live row says its words are.
    var words: String {
        switch self {
        case .everything: t("everything-said-so-far")
        case .lastFewSeconds: t("the-last-few-seconds")
        }
    }
}

struct NoteRow: View {
    let note: NoteSummary

    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(firstLine(note.text))
                    .lineLimit(1)
                Text("\(when(note.created)) · \(wordCount(note.words))")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            IconView(icon: .caretRight, size: 14, color: .tertiaryLabelColor)
        }
        .contentShape(Rectangle())
    }
}

/// The open note, in place of the list.
struct OpenNoteView: View {
    @ObservedObject var model: NotesModel
    let open: OpenNotePage
    @FocusState private var editing: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Button {
                model.send(.close)
            } label: {
                HStack(spacing: 4) {
                    IconView(icon: .caretLeft, size: 14, color: .controlAccentColor)
                    Text(t("all-notes"))
                        .foregroundStyle(Color.accentColor)
                }
            }
            .buttonStyle(.plain)

            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Text(when(open.created))
                    .font(.title2.bold())
                Text("\(wordCount(open.words)) · \(open.saved ? t("saved") : t("saving"))")
                    .foregroundStyle(.secondary)
            }

            if let take = model.page.take {
                VStack(spacing: 10) {
                    TakeRows(model: model, take: take)
                }
                .padding(12)
                .background(Color(nsColor: .quaternarySystemFill), in: RoundedRectangle(cornerRadius: 8))
            }

            TextEditor(text: $model.text)
                .font(.body)
                .focused($editing)
                .scrollContentBackground(.hidden)
                .padding(8)
                .background(Color(nsColor: .textBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
                .overlay(
                    RoundedRectangle(cornerRadius: 8)
                        .strokeBorder(
                            editing ? Color.accentColor : Color(nsColor: .separatorColor),
                            lineWidth: editing ? 2 : 1
                        )
                )
                .onChange(of: model.text) { _, text in model.edited(text) }

            HStack {
                Button(t("delete"), role: .destructive) { model.send(.deleteRequested) }
                    .buttonStyle(.borderedProminent)
                    .tint(.red)
                Spacer()
                if let exported = model.page.exported {
                    Text(t("exported-to", ["path": exported]))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
                Button(t("export")) { model.send(.export) }
                DictateMoreButton(model: model)
            }
        }
        // The list's grouped cards' inset, so the page doesn't shift sideways
        // when a note opens.
        .padding(.horizontal, 28)
        .padding(.vertical)
        .confirmationDialog(
            t("delete-this-note"),
            isPresented: Binding(
                get: { model.page.confirmingDelete },
                set: { shown in if !shown && model.page.confirmingDelete { model.send(.deleteCancelled) } }
            )
        ) {
            Button(t("delete"), role: .destructive) { model.send(.deleteConfirmed) }
            Button(t("cancel"), role: .cancel) { model.send(.deleteCancelled) }
        }
    }
}

/// Dictate more, with why it is off beside it. It waits while any take holds
/// the microphone, and until speech to text is ready.
struct DictateMoreButton: View {
    @ObservedObject var model: NotesModel
    @ObservedObject var header = HeaderModel.shared
    @ObservedObject var microphone = MicrophoneModel.shared

    var body: some View {
        let unready = notReady(feature: .speechToText, readiness: header.header.speechToText)
        let free = model.page.take == nil && !microphone.microphone.recording && unready == nil
        if let unready {
            Text(unready.words)
                .font(.caption)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.trailing)
        }
        Button(t("dictate-more")) { model.send(.dictateMore) }
            .buttonStyle(.borderedProminent)
            .disabled(!free)
    }
}

struct NoticeBanner: View {
    let text: String

    var body: some View {
        Label {
            Text(text)
        } icon: {
            IconView(icon: .warning)
        }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(10)
            .background(.orange.opacity(0.15))
    }
}

extension NotesNotice {
    /// What the page says about it.
    var words: String {
        switch self {
        case let .couldNotRead(message): t("notes-could-not-be-read", ["error": message])
        case let .couldNotSave(message): t("the-note-could-not-be-saved", ["error": message])
        case let .couldNotDelete(message): t("the-note-could-not-be-deleted", ["error": message])
        case let .couldNotExport(message): t("the-note-could-not-be-exported", ["error": message])
        case .noSpeech: t("no-speech-was-heard")
        case .anotherRecording: t("another-recording-is-running")
        case let .recordingFailed(message): t("the-recording-failed", ["error": message])
        }
    }
}

/// "Today, 8:02 AM", "Yesterday, 4:40 PM", or the date and time, in the
/// app's locale.
@MainActor func when(_ seconds: Int64) -> String {
    let date = Date(timeIntervalSince1970: TimeInterval(seconds))
    let calendar = Calendar.current
    let time = date.formatted(Date.FormatStyle(date: .omitted, time: .shortened).locale(appLocale))
    if calendar.isDateInToday(date) { return t("today-at", ["time": time]) }
    if calendar.isDateInYesterday(date) { return t("yesterday-at", ["time": time]) }
    return date.formatted(Date.FormatStyle(date: .abbreviated, time: .shortened).locale(appLocale))
}

/// "1 word", "8 words".
func wordCount(_ words: UInt32) -> String {
    words == 1 ? t("count-1-word") : t("words-count", ["n": "\(words)"])
}

/// A note's first line, which the list shows as its title.
func firstLine(_ text: String) -> String {
    text.split(separator: "\n", omittingEmptySubsequences: true).first.map(String.init) ?? ""
}
