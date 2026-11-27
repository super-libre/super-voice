// SPDX-License-Identifier: GPL-3.0-only
//! The pages, grouped per feature: Speech to text, Text to speech, General.
//!
//! In a wide window, the kit's `Sidebar` with labelled groups, each feature's
//! readiness a pill beside its heading, and "Go to or run…" under the app's
//! name. In a compact window, the same sidebar collapsed to a 48 px rail of
//! icons: the headings hide, so each feature's readiness is a bare dot on its
//! Models icon (the status bar says it in words) and each tooltip names the
//! feature ("Text to speech › Models"). In a narrow window, the expanded
//! groups in a drawer.

use std::rc::Rc;

use gpui_kit::component::badge::Badge;
use gpui_kit::component::button::Button;
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::sidebar::{
    Sidebar, SidebarCollapsible, SidebarHeader, SidebarItem, SidebarMenu, SidebarMenuItem,
};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Collapsible, Icon, IconName, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, ElementId, FontWeight, InteractiveElement as _, IntoElement,
    Keystroke, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
    WeakEntity, Window, div, img, px,
};
use super_voice_app_core::pages::readiness::{Feature, Readiness};

use super::{Shell, version};
use crate::assets::app_icon;
use crate::pages::{Page, Section, SidebarMark};
use crate::t;
use crate::ui::status::{Dot, StatusDot, StatusPill, dot_color};
use crate::ui::width::WindowSize;

/// What the pages' list shows, read from the shell.
#[derive(Clone)]
pub struct Nav {
    /// The page shown; `None` while the Connection page is.
    shown: Option<Page>,
    /// The daemon is down: the pages that need it are dimmed.
    down: bool,
    readiness: [(Feature, Readiness); 2],
    marks: Vec<(Page, SidebarMark)>,
    /// Finish setup is listed: something macOS has to allow is missing.
    setup_row: bool,
}

impl Nav {
    pub fn read(shell: &Shell, cx: &App) -> Self {
        let readiness = shell.shared.readiness.read(cx);
        Self {
            setup_row: shell.setup_row_shown(cx),
            shown: shell.page_shown(cx),
            down: shell.shared.connection.read(cx).is_down(),
            readiness: Feature::ALL.map(|feature| (feature, readiness.of(feature))),
            marks: shell
                .pages
                .iter()
                .filter_map(|(page, handle)| Some((*page, handle.sidebar_mark(cx)?)))
                .collect(),
        }
    }

    fn readiness(&self, feature: Feature) -> Readiness {
        self.readiness
            .iter()
            .find(|(f, _)| *f == feature)
            .map_or(Readiness::Unknown, |(_, r)| *r)
    }

    fn mark(&self, page: Page) -> Option<SidebarMark> {
        self.marks
            .iter()
            .find(|(p, _)| *p == page)
            .map(|(_, mark)| *mark)
    }

    /// The sidebar's groups, each item showing its page through `pick`.
    /// Finish setup's, which has no heading, only while it is listed.
    fn groups(&self, pick: &Pick) -> Vec<NavGroup> {
        Section::ALL
            .into_iter()
            .filter(|section| *section != Section::Setup || self.setup_row)
            .map(|section| NavGroup {
                label: t(section.key()),
                heading: section != Section::Setup,
                feature: section
                    .feature()
                    .map(|feature| (feature, self.readiness(feature))),
                collapsed: false,
                items: section
                    .pages()
                    .map(|page| NavItem {
                        page,
                        active: self.shown == Some(page),
                        disabled: self.down && page.needs_daemon(),
                        dot: page.feature().map(|feature| self.readiness(feature).dot()),
                        mark: self.mark(page),
                        pick: pick.clone(),
                    })
                    .collect(),
            })
            .collect()
    }
}

/// What picking a page does.
type Pick = Rc<dyn Fn(Page, &mut Window, &mut App)>;

/// The sidebar of a wide window, or its rail in a compact one.
pub fn sidebar(shell: &Shell, size: WindowSize, cx: &mut Context<Shell>) -> impl IntoElement {
    let nav = Nav::read(shell, cx);
    let collapsed = size == WindowSize::Compact;
    let weak = cx.weak_entity();
    let pick: Pick = Rc::new(move |page, window, cx| {
        _ = weak.update(cx, |shell, cx| shell.go(page, window, cx));
    });
    let header = if collapsed {
        SidebarHeader::new().child(
            div()
                .w_full()
                .flex()
                .justify_center()
                .child(img(app_icon(px(32.))).size_8()),
        )
    } else {
        let weak = cx.weak_entity();
        SidebarHeader::new().child(
            v_flex()
                .w_full()
                .gap_3()
                .child(brand(cx))
                .child(palette_field("sidebar-palette", weak, cx)),
        )
    };
    Sidebar::new("pages")
        .collapsible(SidebarCollapsible::Icon)
        .collapsed(collapsed)
        .header(header)
        .children(nav.groups(&pick))
}

/// The drawer's list of pages (narrow windows): the sidebar's groups,
/// expanded, without its header, which the drawer's title holds.
pub fn drawer(nav: &Nav, shell: WeakEntity<Shell>, _: &App) -> impl IntoElement {
    let pick: Pick = Rc::new(move |page, window, cx| {
        _ = shell.update(cx, |shell, cx| shell.go(page, window, cx));
    });
    Sidebar::new("drawer-pages")
        .collapsible(SidebarCollapsible::None)
        .w_full()
        .border_0()
        .children(nav.groups(&pick))
}

/// The app's icon, name and version.
pub fn brand(cx: &App) -> impl IntoElement + use<> {
    h_flex()
        .gap_2()
        .items_center()
        .min_w_0()
        .child(img(app_icon(px(32.))).size_8().flex_shrink_0())
        .child(
            v_flex()
                .min_w_0()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(t("app-title")),
                )
                .child(
                    // Normal weight, also in the drawer's title, which is
                    // semibold.
                    div()
                        .text_xs()
                        .font_weight(FontWeight::NORMAL)
                        .text_color(cx.theme().muted_foreground)
                        .child(version()),
                ),
        )
}

/// "Go to or run…" with its keys: opens the palette.
pub fn palette_field(id: &'static str, shell: WeakEntity<Shell>, cx: &App) -> Button {
    let muted = cx.theme().muted_foreground;
    Button::new(id)
        .w_full()
        .child(
            h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .text_color(muted)
                .child(Icon::new(IconName::Search).size_3p5())
                .child(div().flex_1().child(t("go-to-or-run")))
                .child(Kbd::new(
                    Keystroke::parse(super::PALETTE_KEYS).expect("the palette's keys"),
                )),
        )
        .on_click(move |_, window, cx| {
            _ = shell.update(cx, |shell, cx| shell.open_palette(window, cx));
        })
}

/// A group of the sidebar: a feature's pages, or General's.
#[derive(Clone)]
struct NavGroup {
    label: SharedString,
    /// Whether the group shows its heading: Finish setup's has none.
    heading: bool,
    /// The feature and its readiness: its icon before the heading, its pill
    /// after.
    feature: Option<(Feature, Readiness)>,
    collapsed: bool,
    items: Vec<NavItem>,
}

/// A page in the sidebar.
#[derive(Clone)]
struct NavItem {
    page: Page,
    active: bool,
    disabled: bool,
    /// The feature's dot, on a Models icon in the rail.
    dot: Option<Dot>,
    mark: Option<SidebarMark>,
    pick: Pick,
}

impl NavItem {
    /// The kit's item, as the expanded sidebar has it.
    fn menu_item(self) -> SidebarMenuItem {
        let pick = self.pick.clone();
        let page = self.page;
        SidebarMenuItem::new(t(page.key()))
            .icon(page.icon())
            .accessibility_label(page.full_name())
            .active(self.active)
            .disable(self.disabled)
            .on_click(move |_, window, cx| pick(page, window, cx))
            .when_some(self.mark, |item, mark| {
                item.suffix(move |_, cx| mark_element(mark, cx))
            })
            .when(self.mark.is_none() && page.is_preview(), |item| {
                item.suffix(|_, _| crate::ui::page_header::preview_tag().into_any_element())
            })
    }

    /// The rail's icon: the kit's collapsed item, drawn here so the feature's
    /// dot can sit on its icon. Same size, colours and tooltip.
    fn rail_item(self, id: ElementId, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let pick = self.pick.clone();
        let page = self.page;
        let name = page.full_name();
        let icon: Icon = page.icon();
        let badge = match (self.dot, self.mark) {
            (Some(dot), _) => Some(dot_color(dot, cx)),
            (None, Some(SidebarMark::Update)) => Some(theme.info),
            (None, Some(SidebarMark::NeedsSetup)) => Some(dot_color(Dot::Yellow, cx)),
            _ => None,
        };
        let icon = match badge {
            Some(color) => Badge::new()
                .dot()
                .color(color)
                .child(icon)
                .into_any_element(),
            None => icon.into_any_element(),
        };
        let accent = theme.tokens.sidebar_accent;
        let accent_text = theme.sidebar_accent_foreground;
        let muted = theme.muted_foreground;
        div()
            .id(id)
            .w_full()
            .child(
                h_flex()
                    .id("item")
                    .w_full()
                    .h(px(28.))
                    .justify_center()
                    .rounded(theme.radius)
                    .text_sm()
                    .child(icon)
                    .when(self.active, |row| row.bg(accent).text_color(accent_text))
                    .when(!self.active && !self.disabled, |row| {
                        row.hover(|row| row.bg(accent.opacity(0.8)).text_color(accent_text))
                    })
                    .when(self.disabled, |row| row.text_color(muted))
                    .when(!self.disabled, |row| {
                        row.on_click(move |_, window, cx| pick(page, window, cx))
                    })
                    .tooltip(move |window, cx| Tooltip::new(name.clone()).build(window, cx)),
            )
            .into_any_element()
    }
}

/// A page's mark in the expanded sidebar: its count, muted, or a dot.
fn mark_element(mark: SidebarMark, cx: &App) -> AnyElement {
    match mark {
        SidebarMark::Count(0) => div().into_any_element(),
        SidebarMark::Count(count) => div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(count.to_string())
            .into_any_element(),
        SidebarMark::Update => div()
            .flex_shrink_0()
            .size(px(7.))
            .rounded_full()
            .bg(cx.theme().info)
            .into_any_element(),
        SidebarMark::NeedsSetup => StatusDot::new(Dot::Yellow).into_any_element(),
    }
}

impl Collapsible for NavGroup {
    fn is_collapsed(&self) -> bool {
        self.collapsed
    }

    fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }
}

impl SidebarItem for NavGroup {
    fn render(
        self,
        id: impl Into<ElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement {
        let id = id.into();
        if self.collapsed {
            // 12 px between the rail's groups, as between the sidebar's.
            return v_flex()
                .pb_3()
                .gap(px(1.))
                .children(self.items.into_iter().enumerate().map(|(ix, item)| {
                    item.rail_item(ElementId::Name(format!("{id}-{ix}").into()), cx)
                }))
                .into_any_element();
        }
        // The kit's `SidebarGroup` header, with the feature's icon before
        // the heading, which the kit's takes only words for.
        let theme = cx.theme();
        let header = h_flex()
            .flex_shrink_0()
            .h_8()
            .px_2()
            .gap_2()
            .items_center()
            .text_xs()
            .text_color(theme.sidebar_foreground.opacity(0.7))
            .when_some(self.feature, |row, (feature, _)| {
                row.child(super::status_bar::feature_icon(feature).size_3())
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(self.label),
            )
            .when_some(self.feature, |row, (_, readiness)| {
                row.child(StatusPill::new(readiness.dot(), t(readiness.key())))
            });
        let menu = SidebarMenu::new()
            .gap_0p5()
            .children(self.items.into_iter().map(NavItem::menu_item));
        v_flex()
            .when(self.heading, |group| group.child(header))
            .child(menu.render(id, window, cx))
            .into_any_element()
    }
}
