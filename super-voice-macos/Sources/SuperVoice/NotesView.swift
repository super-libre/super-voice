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
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(t("new-note"))
                        Text(t("recording-not-in-this-app-yet"))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button(t("record")) {}
                        .buttonStyle(.borderedProminent)
                        .disabled(true)
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
                Button(t("dictate-more")) {}
                    .buttonStyle(.borderedProminent)
                    .disabled(true)
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
