// SPDX-License-Identifier: GPL-3.0-only
use super::common::page_layout;
use crate::core::app::AppModel;
use crate::state::DaemonStatus;
use crate::ui::messages::{DaemonMessage, Message, ShellMessage};
use cosmic::iced::Alignment;
use cosmic::iced::widget::row;
use cosmic::{
    Element,
    widget::{button, settings, text},
};
use super_voice_app_core::pages::connection::{self as page, Fix, Input};

/// The window's whole body while the daemon cannot be reached, with the nav
/// hidden: the Connection sheet's rows under a "Connection" heading.
pub fn page(app: &AppModel) -> Element<'_, Message> {
    page_layout(
        crate::fl!("connection"),
        settings::view_column(vec![section(app).into()]),
    )
}

/// The View menu's Connection sheet: the daemon's status and socket, and what
/// to do when it cannot be reached.
pub fn sheet(app: &AppModel) -> Element<'_, Message> {
    section(app).into()
}

/// The connection's rows, as the page and the sheet both show them.
fn section(app: &AppModel) -> settings::Section<'_, Message> {
    let status_text = match &app.connection.status {
        DaemonStatus::Connected => crate::fl!("connected"),
        DaemonStatus::Connecting => crate::fl!("connecting"),
        DaemonStatus::Disconnected => crate::fl!("disconnected"),
        DaemonStatus::Error(err) => crate::fl!("connection-error", err = err.as_str()),
        DaemonStatus::Blocked(reason) => {
            crate::fl!("connection-denied", reason = reason.as_str())
        }
    };

    let section = settings::section()
        .add(settings::item(
            crate::fl!("status"),
            text::body(status_text),
        ))
        .add(settings::item(
            crate::fl!("socket-path"),
            text::body(page::socket_path()),
        ));

    // Not reaching the daemon is the one failure this page can actually help
    // with, and it never said how. A red status and nothing to act on is not
    // an answer, so offer what fixes it.
    match page::fix(&app.connection) {
        None => section,
        Some(Fix::Start { command }) => section
            .add(settings::item(
                crate::fl!("no-daemon"),
                text::body(crate::fl!("the-daemon-is-not-running")),
            ))
            .add(settings::item(
                crate::fl!("start-it"),
                row![
                    text::body(command.clone()),
                    button::standard(crate::fl!("copy"))
                        .on_press(Message::Shell(ShellMessage::CopyText(command))),
                ]
                .spacing(cosmic::theme::spacing().space_xs)
                .align_y(Alignment::Center),
            )),
        Some(Fix::TurnOn) => section
            .add(settings::item(
                crate::fl!("no-daemon"),
                text::body(crate::fl!("super-voice-s-background-service-is-switched")),
            ))
            .add(settings::item(
                crate::fl!("turn-it-on"),
                button::standard(crate::fl!("open-login-items"))
                    .on_press(connection_input(Input::OpenLoginItems)),
            )),
        Some(Fix::Register) => section
            .add(settings::item(
                crate::fl!("no-daemon"),
                text::body(crate::fl!("super-voice-s-background-service-is-not")),
            ))
            .add(settings::item(
                crate::fl!("register-it"),
                button::standard(crate::fl!("register"))
                    .on_press(connection_input(Input::Register)),
            )),
        Some(Fix::Reauthorize { restart }) => section
            .add(settings::item(
                crate::fl!("action-required"),
                text::body(crate::fl!(
                    "authorization-denied-restart",
                    command = restart
                )),
            ))
            .add(settings::item(
                "",
                button::standard(crate::fl!("retry-authorization"))
                    .on_press(Message::Daemon(DaemonMessage::RetryAuthorization)),
            )),
    }
}

fn connection_input(input: Input) -> Message {
    Message::Daemon(DaemonMessage::Connection(input))
}
