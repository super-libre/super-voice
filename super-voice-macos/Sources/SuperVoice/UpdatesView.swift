// SPDX-License-Identifier: GPL-3.0-only
// The Updates page: this version and the latest, the daemon's checks, whether
// it checks on its own and offers betas, and the update itself: offered, its
// run, then installed or failed. As in the settings app.

import SuperVoiceCore
import SwiftUI

struct UpdatesView: View {
    @ObservedObject var model: UpdatesModel

    var body: some View {
        let page = model.page
        VStack(spacing: 0) {
            if let notice = page.notice {
                NoticeBanner(text: notice)
            }
            Form {
                Section {
                    LabeledContent(t("current-version"), value: appVersion())
                    LabeledContent(t("latest-version"), value: page.latestVersion ?? "\u{2014}")
                    LabeledContent {
                        Text(page.checkedAt.map(checkedAt) ?? t("never"))
                    } label: {
                        if let error = page.lastCheckError {
                            Labelled(t("last-checked"), error)
                        } else {
                            Text(t("last-checked"))
                        }
                    }
                    LabeledContent(t("check-for-updates")) {
                        Button(page.check.checking ? t("checking-progress") : t("check-now")) {
                            model.send(.checkNow)
                        }
                        .disabled(!page.check.enabled)
                    }
                } header: {
                    Text(t("version"))
                }

                if page.unsupported {
                    Text(t("the-connected-daemon-predates-update-support"))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                } else {
                    Section {
                        Toggle(isOn: Binding(
                            get: { page.autoCheck ?? true },
                            set: { model.send(.autoCheckToggled(on: $0)) }
                        )) {
                            Labelled(t("check-daily"), t("check-daily-description"))
                        }
                        .disabled(page.autoCheck == nil)
                        if let beta = page.beta {
                            Toggle(isOn: Binding(
                                get: { beta.on },
                                set: { model.send(.betaToggled(on: $0)) }
                            )) {
                                Labelled(t("include-betas"), t("include-betas-description"))
                            }
                            .disabled(beta.pending)
                        }
                    } header: {
                        Text(t("automatic-updates"))
                    }

                    if let panel = page.panel {
                        Section {
                            UpdatePanelView(model: model, panel: panel)
                        } header: {
                            Text(t("update"))
                        }
                    }
                }
            }
            .formStyle(.grouped)
        }
        .navigationTitle(t("updates"))
        .onAppear { model.send(.reload) }
    }

    /// When the daemon last checked, to the minute, in local time.
    private func checkedAt(_ seconds: Int64) -> String {
        Date(timeIntervalSince1970: TimeInterval(seconds))
            .formatted(Date.FormatStyle(date: .numeric, time: .shortened).locale(appLocale))
    }
}

/// The update section: the update offered, its run, or how it ended.
struct UpdatePanelView: View {
    @ObservedObject var model: UpdatesModel
    let panel: UpdatePanel

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            switch panel {
            case let .offered(tag, canStart, installCommand):
                Button(t("update-to-tag", ["tag": tag])) { model.send(.startUpdate) }
                    .buttonStyle(.borderedProminent)
                    .disabled(!canStart)
                // No installer for this computer: the terminal command in its
                // place.
                if let installCommand {
                    Text(t("update-no-installer", ["value0": installCommand]))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
            case let .running(phase, fraction, cancellable):
                Text(phase.words)
                if let fraction {
                    ProgressView(value: Double(fraction))
                        .progressViewStyle(.linear)
                }
                if cancellable {
                    Button(t("cancel"), role: .destructive) { model.send(.cancelUpdate) }
                }
            case let .done(restart, error):
                Text(t("update-installed"))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                if restart {
                    HStack {
                        Text(t("restart-super-voice-to-finish-the-update"))
                        Spacer()
                        Button(t("restart")) { model.restart() }
                            .buttonStyle(.borderedProminent)
                    }
                    if let error {
                        NoticeBanner(text: error)
                    } else if model.relaunchFailed {
                        NoticeBanner(text: t("relaunch-failed"))
                    }
                }
                Button(t("dismiss")) { model.send(.dismissRun) }
            case let .failed(error, installCommand):
                NoticeBanner(text: error ?? t("update-failed"))
                // Declining the password prompt is not a dead end: pressing
                // Update again and authorizing is the answer.
                if let installCommand {
                    Text(t("install-command", ["value0": installCommand]))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
                Button(t("dismiss")) { model.send(.dismissRun) }
            }
        }
    }
}

extension UpdatePhase {
    var words: String {
        switch self {
        case .fetchingInstaller: t("downloading-installer-progress")
        case .resolve: t("resolving-progress")
        case .download: t("downloading-update-progress")
        case .verify: t("verifying-progress")
        case .stage: t("staging-progress")
        case .waitingAuth: t("waiting-for-authorization-enter-your-password-in")
        case .install: t("installing-progress")
        case .postInstall: t("finishing-progress")
        }
    }
}
