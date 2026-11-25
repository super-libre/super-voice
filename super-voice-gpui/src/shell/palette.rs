// SPDX-License-Identifier: GPL-3.0-only
//! The command palette: Ctrl+K (Cmd+K on macOS), the sidebar's "Go to or
//! run…", or the title bar's search. It runs a page's command or goes to a
//! page: Commands first (each page's [`PageView::commands`]), then Go to
//! (every page), then the groups pages add (Notes).
//!
//! The kit's `Command` in a `Dialog`, 560 px wide or the window less 32 px.
//! In a narrow window a command's detail goes under its name.
//!
//! [`PageView::commands`]: crate::pages::PageView::commands

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui_kit::component::command::{Command, CommandGroup, CommandItem, CommandState};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::{ActiveTheme as _, Icon, IndexPath, WindowExt as _, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, Context, Entity, Focusable as _, IntoElement, Keystroke,
    ParentElement as _, SharedString, Styled as _, Window, div, px,
};

use super::Shell;
use crate::pages::{Page, PageHandle, PaletteCommand};
use crate::t;
use crate::ui::width::{self, WindowSize};

/// The palette's width, when the window leaves room for it.
const WIDTH: f32 = 560.;

/// What a row does when it is run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// Show the page.
    GoTo(Page),
    /// Show the page and run its command.
    Run(Page, SharedString),
}

/// One row of the palette.
#[derive(Clone)]
pub struct Entry {
    icon: Icon,
    label: SharedString,
    detail: Option<SharedString>,
    keywords: Vec<SharedString>,
    pick: Pick,
}

/// A group of rows, under its heading.
#[derive(Clone)]
pub struct Group {
    label: SharedString,
    entries: Vec<Entry>,
}

/// The palette's groups, from every page.
pub fn entries(pages: &[(Page, PageHandle)], cx: &App) -> Vec<Group> {
    let mut commands = Group {
        label: t("palette-commands"),
        entries: Vec::new(),
    };
    let mut others: Vec<Group> = Vec::new();
    for (page, handle) in pages {
        for command in handle.commands(cx) {
            let PaletteCommand {
                id,
                label,
                icon,
                detail,
                group,
                keywords,
            } = command;
            let entry = Entry {
                icon,
                label,
                detail,
                keywords,
                pick: Pick::Run(*page, id),
            };
            match group {
                None => commands.entries.push(entry),
                Some(label) => match others.iter_mut().find(|g| g.label == label) {
                    Some(group) => group.entries.push(entry),
                    None => others.push(Group {
                        label,
                        entries: vec![entry],
                    }),
                },
            }
        }
    }
    let go_to = Group {
        label: t("palette-go-to"),
        entries: pages
            .iter()
            .map(|(page, _)| Entry {
                icon: page.icon(),
                label: page.full_name(),
                detail: None,
                keywords: vec![t(page.key())],
                pick: Pick::GoTo(*page),
            })
            .collect(),
    };
    [commands, go_to]
        .into_iter()
        .chain(others)
        .filter(|group| !group.entries.is_empty())
        .collect()
}

/// The palette's state, kept while the window is open.
pub struct Palette {
    state: Entity<CommandState>,
    /// The groups shown, while open.
    groups: Rc<RefCell<Vec<Group>>>,
    open: Rc<Cell<bool>>,
}

impl Palette {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            state: cx.new(|cx| CommandState::new(window, cx)),
            groups: Rc::default(),
            open: Rc::default(),
        }
    }

    /// Open the palette with `groups`, or focus it when it is open.
    pub fn open(&mut self, groups: Vec<Group>, window: &mut Window, cx: &mut Context<Shell>) {
        if self.open.get() {
            self.state.read(cx).focus_handle(cx).focus(window, cx);
            return;
        }
        *self.groups.borrow_mut() = groups;
        self.open.set(true);
        self.state
            .update(cx, |state, cx| state.set_query("", window, cx));

        let state = self.state.clone();
        let groups = self.groups.clone();
        let open = self.open.clone();
        let shell = cx.weak_entity();
        let focus_on_mount = Rc::new(Cell::new(true));
        let palette_width = (width::window_width(window) - px(32.)).min(px(WIDTH));
        window.open_dialog(cx, move |dialog, window, _| {
            let state = state.clone();
            let groups = groups.clone();
            let open = open.clone();
            let shell = shell.clone();
            let focus_on_mount = focus_on_mount.clone();
            let narrow = WindowSize::of(window) == WindowSize::Narrow;
            dialog
                .close_button(false)
                .p_0()
                .w(palette_width)
                .on_close({
                    let open = open.clone();
                    move |_, _, _| open.set(false)
                })
                .content(move |content, window, cx| {
                    if focus_on_mount.replace(false) {
                        let state = state.clone();
                        window.defer(cx, move |window, cx| {
                            state.read(cx).focus_handle(cx).focus(window, cx);
                        });
                    }
                    let picks = groups.clone();
                    let shell = shell.clone();
                    let open = open.clone();
                    let command = Command::new(&state)
                        .bordered(false)
                        .rounded(cx.theme().radius_lg)
                        .placeholder(t("go-to-or-run"))
                        .max_h(px(400.))
                        .empty(|_, _, cx| {
                            div()
                                .p_4()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(t("palette-nothing-matches"))
                        })
                        .footer(|_, _, cx| footer(cx))
                        .on_confirm(move |index: IndexPath, window, cx| {
                            let pick = picks
                                .borrow()
                                .get(index.section)
                                .and_then(|group| group.entries.get(index.row))
                                .map(|entry| entry.pick.clone());
                            open.set(false);
                            window.close_dialog(cx);
                            if let Some(pick) = pick {
                                _ = shell.update(cx, |shell, cx| shell.run_pick(pick, window, cx));
                            }
                        });
                    let command = groups.borrow().iter().fold(command, |command, group| {
                        command.group(
                            CommandGroup::new()
                                .label(group.label.clone())
                                .items(group.entries.iter().map(|entry| item(entry, narrow))),
                        )
                    });
                    content.child(command)
                })
        });
    }
}

/// A row: its icon and name, and where it acts on the right (under its name
/// in a narrow window).
fn item(entry: &Entry, narrow: bool) -> CommandItem {
    let icon = entry.icon.clone();
    let label = entry.label.clone();
    let detail = entry.detail.clone();
    CommandItem::new()
        .label(entry.label.clone())
        .keywords(entry.keywords.clone())
        .child(move |_, cx| {
            let muted = cx.theme().muted_foreground;
            let icon = icon.clone().size_3p5().text_color(muted);
            if narrow {
                h_flex()
                    .gap_2()
                    .items_center()
                    .min_w_0()
                    .child(icon)
                    .child(
                        v_flex()
                            .min_w_0()
                            .child(div().truncate().child(label.clone()))
                            .children(
                                detail
                                    .clone()
                                    .map(|detail| div().text_xs().text_color(muted).child(detail)),
                            ),
                    )
                    .into_any_element()
            } else {
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .min_w_0()
                    .child(icon)
                    .child(div().flex_1().min_w_0().truncate().child(label.clone()))
                    .children(detail.clone().map(|detail| {
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(muted)
                            .child(detail)
                    }))
                    .into_any_element()
            }
        })
}

/// The keys under the rows: ↑ ↓ to move, Enter to run.
fn footer(cx: &App) -> impl IntoElement + use<> {
    fn key(key: &'static str) -> Kbd {
        Kbd::new(Keystroke::parse(key).expect("a key the palette names"))
    }
    h_flex()
        .gap_1p5()
        .items_center()
        .px_3p5()
        .py_2()
        .border_t_1()
        .border_color(cx.theme().border)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(key("up"))
        .child(key("down"))
        .child(t("palette-to-move"))
        .child(div().w_2())
        .child(key("enter"))
        .child(t("palette-to-run"))
}
