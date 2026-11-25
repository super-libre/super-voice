// SPDX-License-Identifier: GPL-3.0-only
// A backend's Configure sheet: which dictation context it is sent (for a
// backend that takes one), its secrets, and its options. A secret is saved
// with Save and never shown again; a switch or a pick from a list saves as it
// changes; a text option saves with Save, and Reset puts its default back.
// As in the settings app.

import SuperVoiceCore
import SwiftUI

/// Configure, for one backend: puts its sheet up.
struct ConfigureButton: View {
    let source: String
    @ObservedObject private var model = ConfigureModel.shared

    var body: some View {
        Button(t("configure")) { model.open(source) }
            .sheet(isPresented: Binding(
                get: { model.presented == source },
                set: { shown in if !shown && model.presented == source { model.close() } }
            )) {
                ConfigureSheetView(model: model)
            }
    }
}

/// The sheet over one backend.
struct ConfigureSheetView: View {
    @ObservedObject var model: ConfigureModel

    var body: some View {
        let sheet = model.sheet
        VStack(alignment: .leading, spacing: 0) {
            Text(model.ready ? t("backend-configuration", ["name": sheet.name]) : t("configure"))
                .font(.title2.bold())
                .padding([.top, .horizontal], 20)
            if let notice = sheet.notice {
                NoticeBanner(text: notice)
                    .padding(.top, 10)
            }
            if model.ready {
                Form {
                    if let context = sheet.context {
                        Section {
                            ContextPickerRow(model: model, picker: context)
                        }
                    }
                    Section {
                        if sheet.rows.isEmpty {
                            Text(sheet.context == nil
                                 ? t("this-backend-has-nothing-to-configure")
                                 : t("this-backend-has-no-other-settings"))
                                .foregroundStyle(.secondary)
                        }
                        ForEach(sheet.rows, id: \.name) { row in
                            ConfigureRowView(model: model, row: row)
                        }
                    }
                }
                .formStyle(.grouped)
            } else {
                Spacer()
                HStack {
                    Spacer()
                    ProgressView().controlSize(.small)
                    Text(t("loading-progress"))
                        .foregroundStyle(.secondary)
                    Spacer()
                }
                Spacer()
            }
            HStack {
                Spacer()
                Button(t("done")) { model.close() }
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(.borderedProminent)
            }
            .padding([.horizontal, .bottom], 20)
        }
        .frame(width: 520, height: 480)
    }
}

/// Which dictation context the backend is sent: the active one, none, or one
/// of its own.
struct ContextPickerRow: View {
    @ObservedObject var model: ConfigureModel
    let picker: ContextPicker

    var body: some View {
        Picker(selection: Binding(
            get: { picker.selected },
            set: { model.send(.pickContext(choice: $0)) }
        )) {
            ForEach(picker.options, id: \.choice) { option in
                Text(option.name).tag(option.choice)
            }
        } label: {
            RowLabel(title: t("dictation-context"), hint: picker.hint, required: false)
        }
    }
}

/// One secret or option, with the control its kind takes.
struct ConfigureRowView: View {
    @ObservedObject var model: ConfigureModel
    let row: ConfigureRow

    var body: some View {
        switch row.control {
        case let .secret(configured, _):
            VStack(alignment: .leading, spacing: 8) {
                RowLabel(title: row.label, hint: row.hint, required: row.required)
                if configured {
                    HStack {
                        Text(t("configured"))
                        Spacer()
                        Button(t("remove"), role: .destructive) { model.send(.removeSecret(name: row.name)) }
                    }
                } else {
                    HStack {
                        SecureField("", text: field, prompt: Text(t("enter-api-key")))
                            .labelsHidden()
                            .textFieldStyle(.roundedBorder)
                            .onSubmit { model.send(.saveSecret(name: row.name)) }
                        Button(t("save")) { model.send(.saveSecret(name: row.name)) }
                    }
                }
            }
            .padding(.vertical, 2)
        case let .bool(on):
            // The switch moves once the daemon has stored it.
            Toggle(isOn: Binding(
                get: { on },
                set: { model.send(.toggleOption(name: row.name, on: $0)) }
            )) {
                RowLabel(title: row.label, hint: row.hint, required: false)
            }
            .toggleStyle(.switch)
        case let .choice(choices, selected):
            Picker(selection: Binding<String?>(
                get: { selected },
                set: { value in
                    if let value { model.send(.chooseOption(name: row.name, value: value)) }
                }
            )) {
                // A stored value the backend no longer offers: pick again.
                if selected == nil {
                    Text(t("select")).tag(String?.none)
                }
                ForEach(choices, id: \.self) { choice in
                    Text(choice).tag(String?.some(choice))
                }
            } label: {
                RowLabel(title: row.label, hint: row.hint, required: false)
            }
        case let .text(_, _, overridden):
            VStack(alignment: .leading, spacing: 8) {
                RowLabel(title: row.label, hint: row.hint, required: row.required)
                HStack {
                    TextField("", text: field)
                        .labelsHidden()
                        .textFieldStyle(.roundedBorder)
                        .onSubmit { model.send(.saveOption(name: row.name)) }
                    Button(t("save")) { model.send(.saveOption(name: row.name)) }
                    if overridden {
                        Button(t("reset")) { model.send(.resetOption(name: row.name)) }
                    }
                }
            }
            .padding(.vertical, 2)
        }
    }

    /// The row's field, kept by the model.
    private var field: Binding<String> {
        Binding(
            get: { model.field(row) },
            set: { model.typed(row, $0) }
        )
    }
}

/// A row's title, an accent asterisk when it must be set, and its hint below.
struct RowLabel: View {
    let title: String
    let hint: String?
    let required: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 0) {
                Text(title)
                if required {
                    Text(" *").foregroundStyle(Color.accentColor)
                }
            }
            if let hint, !hint.isEmpty {
                Text(hint)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }
}
