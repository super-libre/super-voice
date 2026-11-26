// SPDX-License-Identifier: GPL-3.0-only
//! "Install manually": a backend from outside the catalog, from a git
//! repository's latest release or a folder on this computer. Check reads the
//! source and shows what it would install, the same facts the inspector
//! shows, before anything is installed; Install commits to it.
//!
//! A sheet on the right, 440 px, or the window's width in a narrow one. The
//! logic is the core's `pages::add_backend`.

use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_kit::component::input::Input as TextField;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Placement, Selectable as _, Sizable as _, WindowExt as _,
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, IntoElement, ParentElement as _, Styled as _, WeakEntity, Window, div, px,
};
use super_voice_app_core::pages::add_backend::{self as add, Input};
use super_voice_app_core::pages::library::{self as page, Installing};
use super_voice_app_core::state::registry::{AddSource, PreviewState};

use super::{LibraryPage, Msg, Sheet, inspector, phase_words};
use crate::effects::CorePage as _;
use crate::icons::Phosphor;
use crate::ui::width::{self, WindowSize};
use crate::{t, t_with};

/// The sheet's width, where the window leaves room for it.
const WIDTH: f32 = 440.;

/// Open the sheet over the Library.
pub fn open(this: WeakEntity<LibraryPage>, window: &mut Window, cx: &mut App) {
    window.open_sheet_at(Placement::Right, cx, move |sheet, window, cx| {
        let Some(page) = this.upgrade() else {
            return sheet;
        };
        let on_close = this.clone();
        let size = if WindowSize::of(window) == WindowSize::Narrow {
            width::window_width(window)
        } else {
            px(WIDTH)
        };
        let page = page.read(cx);
        sheet
            .size(size)
            .title(t("install-manually"))
            .on_close(move |_, _, cx| {
                _ = on_close.update(cx, |page, cx| {
                    if page.sheet == Some(Sheet::Add) {
                        page.sheet = None;
                    }
                    cx.notify();
                });
            })
            .child(body(page, &this, cx))
            .footer(footer(page, &this, cx))
    });
}

fn body(page: &LibraryPage, this: &WeakEntity<LibraryPage>, cx: &App) -> AnyElement {
    let registry = &page.registry;
    let source = registry.add_source;
    let checking = registry.add_preview.is_checking();
    let named = !registry.add_key().is_empty();
    let field = match source {
        AddSource::Repository => TextField::new(&page.url),
        AddSource::Folder => TextField::new(&page.folder),
    };
    let choose = {
        let this = this.clone();
        Button::new("choose")
            .outline()
            .icon(Phosphor::FolderOpen)
            .label(t("choose-progress"))
            .on_click(move |_, window, cx| {
                _ = this.update(cx, |_, cx| LibraryPage::pick_folder(window, cx));
            })
    };
    v_flex()
        .w_full()
        .pb_4()
        .gap_3()
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(t("library-install-manually-body")),
        )
        .child(
            v_flex()
                .gap_1p5()
                .child(div().text_sm().child(t("source")))
                .child(
                    ButtonGroup::new("add-source")
                        .outline()
                        .child(
                            Button::new("repository")
                                .label(t("git-repository"))
                                .selected(source == AddSource::Repository),
                        )
                        .child(
                            Button::new("folder")
                                .label(t("local-folder"))
                                .selected(source == AddSource::Folder),
                        )
                        .on_click({
                            let this = this.clone();
                            move |picked: &Vec<usize>, window, cx| {
                                let source = if picked.contains(&1) {
                                    AddSource::Folder
                                } else {
                                    AddSource::Repository
                                };
                                _ = this.update(cx, |page, cx| {
                                    page.send(Msg::Add(Input::Source(source)), window, cx);
                                });
                            }
                        }),
                ),
        )
        .child(
            h_flex()
                .w_full()
                .gap_2()
                .child(div().flex_1().min_w_0().child(field))
                .when(source == AddSource::Folder, |row| row.child(choose))
                .child(
                    Button::new("check")
                        .outline()
                        .label(if checking {
                            t("checking-progress")
                        } else {
                            t("check")
                        })
                        .loading(checking)
                        .disabled(checking || (!named && source == AddSource::Repository))
                        .on_click(LibraryPage::press(this, Msg::Add(Input::Check))),
                ),
        )
        .child(preview(page, this, cx))
        .into_any_element()
}

/// What the source would install, or why it couldn't be read.
fn preview(page: &LibraryPage, this: &WeakEntity<LibraryPage>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let frame = v_flex()
        .w_full()
        .gap_3()
        .p_3()
        .rounded(theme.radius_lg)
        .border_1()
        .border_color(theme.border);
    let registry = &page.registry;
    match &registry.add_preview {
        PreviewState::Empty => frame
            .bg(theme.muted)
            .child(div().text_sm().child(t("nothing-to-preview-yet")))
            .child(div().text_xs().text_color(theme.muted_foreground).child(
                match registry.add_source {
                    AddSource::Repository => t("paste-a-repository-url-and-press-check"),
                    AddSource::Folder => t("choose-a-folder-that-holds-a-backend"),
                },
            ))
            .into_any_element(),
        PreviewState::Checking(what) => frame
            .bg(theme.muted)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(Spinner::new().small())
                    .child(t_with("reading-source", &[("what", what)])),
            )
            .into_any_element(),
        PreviewState::Failed(message) => frame
            .child(
                gpui_kit::component::alert::Alert::error("add-failed", message.clone())
                    .title(t("library-check-failed")),
            )
            .into_any_element(),
        PreviewState::Ready(backend) => {
            let mut row = page::preview_row(registry, backend);
            // The sheet's install is keyed by what was pointed at.
            row.installing = page::installing(registry, &registry.add_key());
            frame
                .child(inspector::heading(&row, true, cx))
                .child(inspector::body(&row, true, this, cx))
                .into_any_element()
        }
    }
}

/// What Install would do, or how the install it started stands; then
/// Cancel and Install.
fn footer(page: &LibraryPage, this: &WeakEntity<LibraryPage>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let registry = &page.registry;
    let key = registry.add_key();
    let standing = if key.is_empty() {
        Installing::Idle
    } else {
        page::installing(registry, &key)
    };
    let installable = add::installable(registry).map(|b| b.name.clone());
    let running = matches!(standing, Installing::Running { .. });
    let label = match (&standing, &installable) {
        (Installing::Running { phase, .. }, _) => t_with(
            "install-backend-progress",
            &[("value0", &phase_words(*phase))],
        ),
        (_, Some(name)) => t_with("install-backend", &[("value0", name)]),
        _ => t("install"),
    };
    let note = match &standing {
        Installing::Failed(message) => Some(
            div()
                .text_color(theme.danger)
                .child(t_with("failed-detail", &[("err", message)])),
        ),
        Installing::Running {
            fraction: Some(f), ..
        } => Some(div().child(format!("{}%", (f * 100.).round()))),
        Installing::Running { .. } => None,
        Installing::Idle => registry
            .add_preview
            .ready()
            .filter(|_| installable.is_some())
            .map(|backend| {
                div().child(t_with(
                    "backend-version-source",
                    &[
                        ("value0", &backend.version),
                        (
                            "where_from",
                            &match registry.add_source {
                                AddSource::Repository => t("from-its-latest-release"),
                                AddSource::Folder => t("copied-from-disk"),
                            },
                        ),
                    ],
                ))
            }),
    };
    h_flex()
        .w_full()
        .gap_2()
        .items_center()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(theme.muted_foreground)
                .children(note),
        )
        .child(
            Button::new("add-cancel")
                .ghost()
                .label(t("cancel"))
                .on_click(|_, window, cx| window.close_sheet(cx)),
        )
        .child(
            Button::new("add-install")
                .primary()
                .icon(Phosphor::DownloadSimple)
                .label(label)
                .loading(running)
                .disabled(installable.is_none() || running)
                .on_click(LibraryPage::press(this, Msg::Add(Input::Install))),
        )
        .into_any_element()
}
