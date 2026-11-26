// SPDX-License-Identifier: GPL-3.0-only
//! General › Library: the backends installed and the registry's catalog, in
//! one table so they can be compared, with an inspector for the one picked.
//!
//! Someone comparing backends before installing one reads the table; the
//! inspector says what a backend runs as, what it reaches and needs, what it
//! runs on, and its models by kind, before it is installed. From it an
//! installed backend is updated, configured or uninstalled, and one from the
//! catalog installed, with its progress in its row. "Install manually…"
//! installs one from a git repository or a folder (`add_sheet.rs`).
//!
//! The inspector sits beside the table while the content is 760 px wide, is
//! a sheet over it from 600, and below that the table becomes a list in two
//! sections, Installed and Available, with the inspector as a page of its
//! own behind a back button.
//!
//! The logic is the core's: `pages::library` (the table's rows, the filters,
//! installs, the uninstall question) and `pages::add_backend` (the sheet).

mod add_sheet;
mod inspector;
mod table;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input as TextField, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::select::{Select, SelectEvent, SelectItem, SelectState};
use gpui_kit::component::table::{DataTable, TableEvent, TableState};
use gpui_kit::component::{
    ActiveTheme as _, IndexPath, Placement, Selectable as _, Sizable as _, WindowExt, h_flex,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, InteractiveElement as _,
    IntoElement, ParentElement as _, PathPromptOptions, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, WeakEntity, Window, div, px,
};
use super_voice_app_core::events::Event;
use super_voice_app_core::pages::add_backend::{self as add};
use super_voice_app_core::pages::library::{
    self as page, Effect, Input, LibraryState, Row, RowState, Runs,
};
use super_voice_app_core::pages::readiness::Feature;
use super_voice_app_core::rules::backends::ModelKind;
use super_voice_app_core::state::registry::{
    FeatureFilter, RefreshOutcome, RegistryState, RoleFilter,
};
use super_voice_shared::registry::events::InstallPhase;

use crate::effects::{self, CorePage, Runner, runner};
use crate::pages::{Page, PageView, PaletteCommand, SidebarMark};
use crate::shared::Shared;
use crate::ui::empty_state::EmptyState;
use crate::ui::list_detail::ListDetail;
use crate::ui::page_header::PageHeader;
use crate::ui::status::{Dot, StatusPill};
use crate::ui::toast::{self, Toast};
use crate::ui::width::{self, Inspector};
use crate::{t, t_with};

use self::table::Rows;
use crate::icons::Phosphor;

/// The inspector's width beside the table, and as a sheet.
const INSPECTOR: f32 = 340.;

/// What the page takes in: the Library's input, or the "Install manually"
/// sheet's.
#[derive(Debug, Clone)]
pub enum Msg {
    Library(Input),
    Add(add::Input),
}

/// What the page does: the Library's effect, or the sheet's.
#[derive(Debug, Clone)]
pub enum Fx {
    Library(Effect),
    Add(add::Effect),
}

/// Which of the page's sheets is up: the kit opens one at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sheet {
    /// The inspector, over a table too narrow to have it beside.
    Inspector,
    /// "Install manually".
    Add,
}

/// One choice in a filter's Select.
#[derive(Debug, Clone, PartialEq)]
struct Choice<T> {
    title: SharedString,
    value: T,
}

impl<T: Clone + PartialEq + 'static> SelectItem for Choice<T> {
    type Value = T;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &T {
        &self.value
    }
}

type ChoiceState<T> = Entity<SelectState<Vec<Choice<T>>>>;

pub struct LibraryPage {
    registry: RegistryState,
    library: LibraryState,
    shared: Shared,
    table: Entity<TableState<Rows>>,
    search: Entity<InputState>,
    feature: ChoiceState<FeatureFilter>,
    kind: ChoiceState<RoleFilter>,
    runs: ChoiceState<Option<bool>>,
    url: Entity<InputState>,
    folder: Entity<InputState>,
    sheet: Option<Sheet>,
    /// The uninstall question is up.
    asking: bool,
    /// The daemon has answered: what the page shows came from it.
    reached: bool,
    /// The row the page itself selected in the table, whose selection
    /// event is not a pick.
    quiet_select: Option<usize>,
    _subscriptions: Vec<Subscription>,
}

fn feature_choices() -> Vec<Choice<FeatureFilter>> {
    vec![
        Choice {
            title: t("library-all-features"),
            value: FeatureFilter::All,
        },
        Choice {
            title: t("speech-to-text"),
            value: FeatureFilter::SpeechToText,
        },
        Choice {
            title: t("text-to-speech"),
            value: FeatureFilter::TextToSpeech,
        },
    ]
}

fn kind_choices() -> Vec<Choice<RoleFilter>> {
    vec![
        Choice {
            title: t("library-all-kinds"),
            value: RoleFilter::All,
        },
        Choice {
            title: t("transcription"),
            value: RoleFilter::Transcription,
        },
        Choice {
            title: t("post-processing"),
            value: RoleFilter::PostProcessing,
        },
    ]
}

fn runs_choices() -> Vec<Choice<Option<bool>>> {
    vec![
        Choice {
            title: t("library-local-or-cloud"),
            value: None,
        },
        Choice {
            title: t("local"),
            value: Some(false),
        },
        Choice {
            title: t("cloud"),
            value: Some(true),
        },
    ]
}

impl PageView for LibraryPage {
    fn new(_: Page, shared: &Shared, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let table = cx.new(|cx| {
            TableState::new(Rows::default(), window, cx)
                .row_selectable(true)
                .col_selectable(false)
                .sortable(false)
                .col_movable(false)
                .col_resizable(false)
        });
        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t("search-backends-or-models-progress"))
        });
        let url = cx
            .new(|cx| InputState::new(window, cx).placeholder("https://github.com/owner/backend"));
        let folder = cx.new(|cx| InputState::new(window, cx).placeholder(t("library-folder-path")));
        let first = Some(IndexPath::default());
        let feature = cx.new(|cx| SelectState::new(feature_choices(), first, window, cx));
        let kind = cx.new(|cx| SelectState::new(kind_choices(), first, window, cx));
        let runs = cx.new(|cx| SelectState::new(runs_choices(), first, window, cx));
        let subscriptions = vec![
            cx.subscribe_in(&table, window, Self::on_table),
            cx.subscribe_in(&search, window, |this, state, event, window, cx| {
                if let InputEvent::Change = event {
                    let text = state.read(cx).value().to_string();
                    this.send(Msg::Library(Input::Search(text)), window, cx);
                }
            }),
            cx.subscribe_in(&url, window, |this, state, event, window, cx| match event {
                InputEvent::Change => {
                    let text = state.read(cx).value().to_string();
                    this.send(Msg::Add(add::Input::Url(text)), window, cx);
                }
                InputEvent::PressEnter { .. } => this.send(Msg::Add(add::Input::Check), window, cx),
                _ => {}
            }),
            cx.subscribe_in(
                &folder,
                window,
                |this, state, event, window, cx| match event {
                    InputEvent::Change => {
                        let text = state.read(cx).value().to_string();
                        this.send(Msg::Add(add::Input::Folder(text)), window, cx);
                    }
                    InputEvent::PressEnter { .. } => {
                        this.send(Msg::Add(add::Input::Check), window, cx);
                    }
                    _ => {}
                },
            ),
            cx.subscribe_in(&feature, window, |this, _, event, window, cx| {
                let SelectEvent::Confirm(Some(value)) = event else {
                    return;
                };
                this.send(Msg::Library(Input::Feature(*value)), window, cx);
            }),
            cx.subscribe_in(&kind, window, |this, _, event, window, cx| {
                let SelectEvent::Confirm(Some(value)) = event else {
                    return;
                };
                this.send(Msg::Library(Input::Role(*value)), window, cx);
            }),
            cx.subscribe_in(&runs, window, |this, _, event, window, cx| {
                let SelectEvent::Confirm(Some(value)) = event else {
                    return;
                };
                this.send(Msg::Library(Input::Online(*value)), window, cx);
            }),
        ];
        Self {
            registry: RegistryState::default(),
            library: LibraryState::default(),
            shared: shared.clone(),
            table,
            search,
            feature,
            kind,
            runs,
            url,
            folder,
            sheet: None,
            asking: false,
            reached: false,
            quiet_select: None,
            _subscriptions: subscriptions,
        }
    }

    fn reached(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reached = true;
        self.send(Msg::Library(Input::Reload), window, cx);
    }

    fn shown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A key stored on the Configure sheet, a backend installed from the
        // command line: what the table says may have moved on.
        if self.reached && !self.shared.connection.read(cx).is_down() {
            self.send(Msg::Library(Input::Reload), window, cx);
        }
    }

    fn heard(&mut self, event: &Event, window: &mut Window, cx: &mut Context<Self>) {
        if let Event::Install(event) = event {
            self.send(Msg::Library(Input::Event(event.clone())), window, cx);
        }
    }

    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if width::inspector(width::content(window)) != Inspector::Page
            || self.library.selected.is_none()
        {
            return false;
        }
        self.send(Msg::Library(Input::Select(None)), window, cx);
        true
    }

    fn commands(&self, _: &App) -> Vec<PaletteCommand> {
        let group = t("library");
        let mut commands = vec![
            PaletteCommand::new(
                "install-manually",
                t("install-manually-progress"),
                Phosphor::Package,
            )
            .group(group.clone())
            .keywords([t("git-repository"), t("local-folder")]),
            PaletteCommand::new(
                "refresh-catalog",
                t("refresh-catalog"),
                Phosphor::ArrowsClockwise,
            )
            .group(group.clone()),
        ];
        for row in self.all_rows() {
            if let Some(version) = &row.update_to {
                commands.push(
                    PaletteCommand::new(
                        format!("update:{}", row.source),
                        t_with(
                            "library-update-backend",
                            &[("backend", &row.name), ("version", version)],
                        ),
                        Phosphor::DownloadSimple,
                    )
                    .group(group.clone()),
                );
            }
            if row.installed {
                commands.push(
                    PaletteCommand::new(
                        format!("configure:{}", row.source),
                        t_with("library-configure-backend", &[("backend", &row.name)]),
                        Phosphor::Gear,
                    )
                    .group(group.clone()),
                );
            } else if row.installable() {
                commands.push(
                    PaletteCommand::new(
                        format!("install:{}", row.source),
                        t_with("install-backend", &[("value0", &row.name)]),
                        Phosphor::DownloadSimple,
                    )
                    .group(group.clone())
                    .keywords(
                        row.models
                            .iter()
                            .flat_map(|g| g.names.clone())
                            .map(Into::into),
                    ),
                );
            }
        }
        commands
    }

    fn run(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        match id.split_once(':') {
            None if id == "install-manually" => self.open_add(window, cx),
            None if id == "refresh-catalog" => {
                self.send(Msg::Library(Input::Refresh { index: true }), window, cx);
            }
            Some(("update", source)) => {
                self.pick(source, window, cx);
                self.send(Msg::Library(Input::Update(source.into())), window, cx);
            }
            Some(("install", source)) => {
                self.pick(source, window, cx);
                self.send(Msg::Library(Input::Install(source.into())), window, cx);
            }
            Some(("configure", source)) => self.configure(source, window, cx),
            _ => {}
        }
    }

    fn sidebar_mark(&self, _: &App) -> Option<SidebarMark> {
        (page::updates_ready(&self.registry, &self.library) > 0).then_some(SidebarMark::Update)
    }

    #[cfg(feature = "screenshot")]
    fn pretend(&mut self, scenes: &[&str], window: &mut Window, cx: &mut Context<Self>) {
        self.pretend_scenes(scenes, window, cx);
    }
}

impl CorePage for LibraryPage {
    type Input = Msg;
    type Effect = Fx;

    fn send(&mut self, input: Msg, window: &mut Window, cx: &mut Context<Self>) {
        let effects: Vec<Fx> = match input {
            Msg::Library(input) => page::update(&mut self.registry, &mut self.library, input)
                .into_iter()
                .map(Fx::Library)
                .collect(),
            Msg::Add(input) => add::update(&mut self.registry, input)
                .into_iter()
                .map(Fx::Add)
                .collect(),
        };
        effects::run_effects(self, effects, window, cx);
        self.sync(window, cx);
        // A sheet open over the page is drawn by the window's root from the
        // page's state: draw it again for what changed.
        if self.sheet.is_some() {
            window.refresh();
        }
        cx.notify();
    }

    fn effect(
        &mut self,
        effect: Fx,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Runner<Msg>> {
        let library = |work| Some(runner(async move { Msg::Library(work.await) }));
        match effect {
            Fx::Library(effect) => match effect {
                Effect::FetchCatalog { refresh_index } => Some(runner(async move {
                    Msg::Library(page::run_catalog(refresh_index).await)
                })),
                Effect::Install(source) => Some(runner(async move {
                    Msg::Library(page::run_install(source).await)
                })),
                Effect::Update(source) => Some(runner(async move {
                    Msg::Library(page::run_update(source).await)
                })),
                Effect::Uninstall(source) => Some(runner(async move {
                    Msg::Library(page::run_uninstall(source).await)
                })),
                Effect::ReloadBackends => {
                    Some(runner(
                        async move { Msg::Library(page::run_backends().await) },
                    ))
                }
                // Both: the installed backends and the catalog each carry
                // half of what an install changed.
                Effect::ReloadCatalogs => {
                    effects::run(
                        runner(async move { Msg::Library(page::run_backends().await) }),
                        window,
                        cx,
                    )
                    .detach();
                    Some(runner(async move {
                        Msg::Library(page::run_catalog(false).await)
                    }))
                }
                Effect::ReadKeys { source, required } => Some(runner(async move {
                    Msg::Library(page::run_keys(source, required).await)
                })),
                Effect::ReadInUse(source) => Some(runner(async move {
                    Msg::Library(page::run_in_use(source).await)
                })),
                Effect::Installed { source } => {
                    let name = self.name_of(&source);
                    toast::notice::<Self>(
                        window,
                        cx,
                        false,
                        Some(Toast::success(t_with(
                            "library-installed",
                            &[("backend", &name)],
                        ))),
                    );
                    // The sheet's work is done when what it previewed is in.
                    self.send(Msg::Add(add::Input::Installed(source)), window, cx);
                    None
                }
                Effect::InstallFailed { source, message } => {
                    let name = self.name_of(&source);
                    toast::notice::<Self>(
                        window,
                        cx,
                        false,
                        Some(
                            Toast::error(message)
                                .title(t_with("library-install-failed", &[("backend", &name)])),
                        ),
                    );
                    None
                }
            },
            Fx::Add(effect) => match effect {
                add::Effect::PickFolder => {
                    Self::pick_folder(window, cx);
                    None
                }
                add::Effect::Preview(target) => {
                    Some(runner(
                        async move { Msg::Add(add::run_preview(target).await) },
                    ))
                }
                add::Effect::Install(target) => library(Box::pin(add::run_install(target))),
                add::Effect::Close => {
                    if self.sheet == Some(Sheet::Add) {
                        self.sheet = None;
                        window.close_sheet(cx);
                    }
                    None
                }
            },
        }
    }
}

impl LibraryPage {
    /// Every row, whatever the toolbar filters out, for the palette: a
    /// command for every backend, whatever the table shows.
    fn all_rows(&self) -> Vec<Row> {
        let mut registry = self.registry.clone();
        page::clear_filters(&mut registry);
        registry.filters.include_incompatible = false;
        page::rows(&registry, &self.library)
    }

    /// A backend's name, by its source: installed, in the catalog, or the
    /// source itself (a repository or a folder).
    fn name_of(&self, source: &str) -> String {
        page::row(&self.registry, &self.library, source)
            .map(|row| row.name)
            .or_else(|| self.registry.add_preview.ready().map(|b| b.name.clone()))
            .unwrap_or_else(|| source.to_string())
    }

    /// A press that sends `msg`, from anywhere: the page, a sheet, a dialog.
    fn press(
        this: &WeakEntity<Self>,
        msg: Msg,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        let this = this.clone();
        move |_, window, cx| {
            _ = this.update(cx, |page, cx| page.send(msg.clone(), window, cx));
        }
    }

    /// Pick `source` in the table, and show its inspector.
    fn pick(&mut self, source: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.send(
            Msg::Library(Input::Select(Some(source.to_string()))),
            window,
            cx,
        );
        if width::inspector(width::content(window)) == Inspector::Sheet {
            self.open_inspector(window, cx);
        }
    }

    /// The table's rows, and its selection, as the core has them now.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rows = page::rows(&self.registry, &self.library);
        let selected = self
            .library
            .selected
            .as_ref()
            .and_then(|source| rows.iter().position(|row| &row.source == source));
        let columns = table::columns(width::content(window));
        let quiet = &mut self.quiet_select;
        self.table.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            let relayout = delegate.columns != columns;
            delegate.rows = rows;
            delegate.columns = columns;
            if relayout {
                table.refresh(cx);
            }
            if table.selected_row() != selected {
                match selected {
                    Some(ix) => {
                        *quiet = Some(ix);
                        table.set_selected_row(ix, cx);
                    }
                    None => table.clear_selection(cx),
                }
            }
            cx.notify();
        });
        // The inspector's sheet goes when its backend does, or when the
        // window grows wide enough to have it beside the table.
        if self.sheet == Some(Sheet::Inspector)
            && (page::selected(&self.registry, &self.library).is_none()
                || width::inspector(width::content(window)) != Inspector::Sheet)
        {
            self.sheet = None;
            window.defer(cx, WindowExt::close_sheet);
        }
        if self.library.pending_uninstall.is_some() && !self.asking {
            self.ask_uninstall(window, cx);
        }
        self.sync_fields(window, cx);
    }

    /// The sheet's fields, where the core changed them: emptied once what
    /// they named is installed.
    fn sync_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let url = self.registry.custom_repo_input.clone();
        if self.url.read(cx).value().as_ref() != url {
            self.url
                .update(cx, |state, cx| state.set_value(url, window, cx));
        }
        let folder = self.registry.add_folder.clone();
        if self.folder.read(cx).value().as_ref() != folder {
            self.folder
                .update(cx, |state, cx| state.set_value(folder, window, cx));
        }
    }

    fn on_table(
        &mut self,
        table: &Entity<TableState<Rows>>,
        event: &TableEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TableEvent::SelectRow(ix) | TableEvent::DoubleClickedRow(ix) => {
                // The page selected it to match the core: not a pick.
                if matches!(event, TableEvent::SelectRow(_)) && self.quiet_select == Some(*ix) {
                    self.quiet_select = None;
                    return;
                }
                let Some(source) = table
                    .read(cx)
                    .delegate()
                    .rows
                    .get(*ix)
                    .map(|r| r.source.clone())
                else {
                    return;
                };
                if self.library.selected.as_deref() != Some(source.as_str()) {
                    self.send(Msg::Library(Input::Select(Some(source))), window, cx);
                }
                // A row picked, by a click or the keyboard, opens its
                // inspector where it doesn't fit beside the table.
                if width::inspector(width::content(window)) == Inspector::Sheet {
                    self.open_inspector(window, cx);
                }
            }
            // Escape in the table: nothing picked. A selection cleared
            // because its row was filtered out keeps its inspector.
            TableEvent::ClearSelection => {
                let shown = self.library.selected.as_ref().is_some_and(|source| {
                    table
                        .read(cx)
                        .delegate()
                        .rows
                        .iter()
                        .any(|row| &row.source == source)
                });
                if shown {
                    self.send(Msg::Library(Input::Select(None)), window, cx);
                }
            }
            _ => {}
        }
    }

    /// Open the inspector as a sheet over the table.
    fn open_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sheet == Some(Sheet::Inspector) {
            return;
        }
        self.sheet = Some(Sheet::Inspector);
        let this = cx.weak_entity();
        window.open_sheet_at(Placement::Right, cx, move |sheet, _, cx| {
            let Some(page) = this.upgrade() else {
                return sheet;
            };
            let on_close = this.clone();
            let sheet = sheet.size(px(INSPECTOR)).on_close(move |_, _, cx| {
                _ = on_close.update(cx, |page, cx| {
                    if page.sheet == Some(Sheet::Inspector) {
                        page.sheet = None;
                    }
                    cx.notify();
                });
            });
            let page = page.read(cx);
            let Some(row) = page::selected(&page.registry, &page.library) else {
                return sheet;
            };
            sheet
                .title(inspector::heading(&row, false, cx))
                .child(
                    v_flex()
                        .pb_4()
                        .gap_3()
                        .child(inspector::body(&row, true, &this, cx)),
                )
                .footer(inspector::actions(&row, &this, cx))
        });
        cx.notify();
    }

    /// Ask before uninstalling, naming the backend.
    fn ask_uninstall(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.asking = true;
        let this = cx.weak_entity();
        window.open_alert_dialog(cx, move |alert, _, cx| {
            let Some(page) = this.upgrade() else {
                return alert;
            };
            let page = page.read(cx);
            let Some(source) = page.library.pending_uninstall.clone() else {
                return alert;
            };
            let name = page.name_of(&source);
            let body = if page.library.pending_in_use {
                format!(
                    "{} {}",
                    t("uninstall-backend-in-use"),
                    t("uninstall-backend-body")
                )
            } else {
                t("uninstall-backend-body").to_string()
            };
            let ok = this.clone();
            let cancel = this.clone();
            let closed = this.clone();
            alert
                .icon(gpui_kit::component::Icon::new(Phosphor::Warning))
                .title(t_with("uninstall-backend-question", &[("backend", &name)]))
                .description(body)
                .ok_text(t("uninstall"))
                .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                .cancel_text(t("cancel"))
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    let source = source.clone();
                    _ = ok.update(cx, |page, cx| {
                        page.send(Msg::Library(Input::Uninstall(source)), window, cx);
                    });
                    true
                })
                .on_cancel(move |_, window, cx| {
                    _ = cancel.update(cx, |page, cx| {
                        page.send(Msg::Library(Input::CancelUninstall), window, cx);
                    });
                    true
                })
                .on_close(move |_, window, cx| {
                    _ = closed.update(cx, |page, cx| {
                        page.asking = false;
                        if page.library.pending_uninstall.is_some() {
                            page.send(Msg::Library(Input::CancelUninstall), window, cx);
                        }
                    });
                })
        });
    }

    /// Open "Install manually".
    fn open_add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sheet == Some(Sheet::Add) {
            return;
        }
        if self.sheet.is_some() {
            window.close_sheet(cx);
        }
        self.sheet = Some(Sheet::Add);
        add_sheet::open(cx.weak_entity(), window, cx);
        cx.notify();
    }

    /// The platform's folder picker, for "Install manually".
    fn pick_folder(window: &mut Window, cx: &mut Context<Self>) {
        let answer = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(t("choose-backend-folder")),
        });
        cx.spawn_in(window, async move |this, cx| {
            let path = match answer.await {
                Ok(Ok(Some(paths))) => paths
                    .into_iter()
                    .next()
                    .map(|path| path.display().to_string()),
                Ok(Err(e)) => {
                    log::warn!("The folder picker failed: {e}");
                    None
                }
                Ok(Ok(None)) | Err(_) => None,
            };
            _ = this.update_in(cx, |page, window, cx| {
                page.send(Msg::Add(add::Input::Picked(path)), window, cx);
            });
        })
        .detach();
    }

    /// Configure `source`: the Configure sheet the Models page opens.
    fn configure(&mut self, source: &str, window: &mut Window, cx: &mut Context<Self>) {
        // The Models page of the backend's feature owns the one Configure
        // sheet; it opens it over `source`.
        let feature = page::row(&self.registry, &self.library, source)
            .and_then(|row| row.features.first().copied())
            .unwrap_or(Feature::SpeechToText);
        if self.sheet.is_some() {
            self.sheet = None;
            window.close_sheet(cx);
        }
        let models = match feature {
            Feature::SpeechToText => Page::SttModels,
            Feature::TextToSpeech => Page::TtsModels,
        };
        self.shared
            .nav
            .go_and_run(models, format!("configure:{source}"), window, cx);
    }

    /// The toolbar above the table: the search, then the filters as three
    /// Selects where there is room, or one Filters menu.
    fn toolbar(
        &self,
        layout: Inspector,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let this = cx.weak_entity();
        let padding = width::page_padding(window);
        let search = TextField::new(&self.search)
            .prefix(gpui_kit::component::Icon::new(Phosphor::MagnifyingGlass).small())
            .cleanable(true);
        let refresh = Button::new("refresh")
            .ghost()
            .icon(Phosphor::ArrowsClockwise)
            .tooltip(t("refresh-catalog"))
            .loading(self.registry.last_refresh.is_none() && !self.registry.backends.is_empty())
            .on_click(Self::press(
                &this,
                Msg::Library(Input::Refresh { index: true }),
            ));
        let bar = h_flex()
            .w_full()
            .flex_wrap()
            .gap_2()
            .items_center()
            .px(padding)
            .py_2p5()
            .border_b_1()
            .border_color(cx.theme().border);
        match layout {
            Inspector::Beside => {
                let filters = &self.registry.filters;
                bar.flex_nowrap()
                    .child(div().flex_1().min_w(px(160.)).max_w(px(260.)).child(search))
                    .child(
                        div()
                            .w(px(140.))
                            .flex_shrink_0()
                            .child(Select::new(&self.feature).accessibility_label(t("feature"))),
                    )
                    .when(filters.feature.has_kinds(), |bar| {
                        bar.child(
                            div()
                                .w(px(140.))
                                .flex_shrink_0()
                                .child(Select::new(&self.kind).accessibility_label(t("kind"))),
                        )
                    })
                    .child(
                        div()
                            .w(px(140.))
                            .flex_shrink_0()
                            .child(Select::new(&self.runs).accessibility_label(t("runs-on"))),
                    )
                    .child(
                        div().flex_shrink_0().child(
                            Checkbox::new("incompatible")
                                .label(t("show-incompatible"))
                                .checked(filters.include_incompatible)
                                .on_change({
                                    let this = this.clone();
                                    move |on: &bool, window, cx| {
                                        _ = this.update(cx, |page, cx| {
                                            page.send(
                                                Msg::Library(Input::IncludeIncompatible(*on)),
                                                window,
                                                cx,
                                            );
                                        });
                                    }
                                }),
                        ),
                    )
                    .child(div().flex_1())
                    .child(refresh)
                    .into_any_element()
            }
            Inspector::Sheet => bar
                .child(div().flex_1().min_w(px(200.)).child(search))
                .child(self.filters_menu(false, cx))
                .child(refresh)
                .into_any_element(),
            Inspector::Page => bar
                .flex_nowrap()
                .child(div().flex_1().min_w_0().child(search))
                .child(self.filters_menu(true, cx))
                .child(Self::more_menu(cx))
                .into_any_element(),
        }
    }

    /// A narrow window's "…" menu: Install manually… and Refresh catalog.
    fn more_menu(cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.weak_entity();
        Button::new("more")
            .ghost()
            .icon(Phosphor::DotsThreeVertical)
            .tooltip(t("library-more"))
            .dropdown_menu(move |menu: PopupMenu, _, _| {
                menu.item(
                    PopupMenuItem::new(t("install-manually-progress"))
                        .icon(Phosphor::Package)
                        .on_click({
                            let this = this.clone();
                            move |_, window, cx| {
                                _ = this.update(cx, |page, cx| page.open_add(window, cx));
                            }
                        }),
                )
                .item(
                    PopupMenuItem::new(t("refresh-catalog"))
                        .icon(Phosphor::ArrowsClockwise)
                        .on_click(Self::press(
                            &this,
                            Msg::Library(Input::Refresh { index: true }),
                        )),
                )
            })
    }

    /// The filters in one menu, for a toolbar with no room for three
    /// Selects: the feature, its kind under speech to text, where it runs,
    /// and incompatible backends. Its button counts the filters set.
    fn filters_menu(&self, icon_only: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.weak_entity();
        let filters = self.registry.filters.clone();
        let set = page::filters_set(&self.registry);
        let label = if set == 0 {
            t("library-filters")
        } else {
            t_with("library-filters-count", &[("count", &set.to_string())])
        };
        let button = Button::new("filters")
            .icon(Phosphor::FunnelSimple)
            .selected(set > 0)
            .tooltip(label.clone());
        let button = if icon_only {
            button.ghost()
        } else {
            button.outline().label(label).dropdown_caret(true)
        };
        button.dropdown_menu(move |menu: PopupMenu, _, _| {
            let item = |label: SharedString, checked: bool, msg: Msg| {
                PopupMenuItem::new(label)
                    .checked(checked)
                    .on_click(Self::press(&this, msg))
            };
            let mut menu = menu.label(t("feature"));
            for choice in feature_choices() {
                menu = menu.item(item(
                    choice.title,
                    filters.feature == choice.value,
                    Msg::Library(Input::Feature(choice.value)),
                ));
            }
            if filters.feature.has_kinds() {
                menu = menu.separator().label(t("kind"));
                for choice in kind_choices() {
                    menu = menu.item(item(
                        choice.title,
                        filters.role == choice.value,
                        Msg::Library(Input::Role(choice.value)),
                    ));
                }
            }
            menu = menu.separator().label(t("library-runs"));
            for choice in runs_choices() {
                menu = menu.item(item(
                    choice.title,
                    filters.online == choice.value,
                    Msg::Library(Input::Online(choice.value)),
                ));
            }
            menu.separator().item(item(
                t("show-incompatible"),
                filters.include_incompatible,
                Msg::Library(Input::IncludeIncompatible(!filters.include_incompatible)),
            ))
        })
    }

    /// Keep the three Selects showing the core's filters, which the Filters
    /// menu and Clear filters change too.
    fn sync_selects(&self, window: &mut Window, cx: &mut Context<Self>) {
        let filters = self.registry.filters.clone();
        if self.feature.read(cx).selected_value() != Some(&filters.feature) {
            self.feature.update(cx, |state, cx| {
                state.set_selected_value(&filters.feature, window, cx);
            });
        }
        if self.kind.read(cx).selected_value() != Some(&filters.role) {
            self.kind.update(cx, |state, cx| {
                state.set_selected_value(&filters.role, window, cx);
            });
        }
        if self.runs.read(cx).selected_value() != Some(&filters.online) {
            self.runs.update(cx, |state, cx| {
                state.set_selected_value(&filters.online, window, cx);
            });
        }
    }

    /// Under the table: how many it lists, and when the catalog was made.
    fn summary(&self, count: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.weak_entity();
        let registry = &self.registry;
        let updated = registry.generated_at.as_deref().map_or_else(
            || t("catalog-not-loaded"),
            |at| t_with("catalog-updated", &[("t", &page::catalog_time(at))]),
        );
        let mut line = h_flex()
            .flex_wrap()
            .gap_x_2()
            .gap_y_1()
            .items_center()
            .px_3()
            .py_2p5()
            .text_xs()
            .text_color(cx.theme().muted_foreground);
        match &registry.last_refresh {
            None => {
                line = line.child(t("loading-registry-progress"));
            }
            Some(RefreshOutcome::Failed(e)) => {
                line = line
                    .child(
                        div()
                            .text_color(cx.theme().danger)
                            .child(t_with("registry-unreachable", &[("e", e)])),
                    )
                    .child(
                        Button::new("retry")
                            .xsmall()
                            .outline()
                            .label(t("retry"))
                            .on_click(Self::press(
                                &this,
                                Msg::Library(Input::Refresh { index: true }),
                            )),
                    );
            }
            Some(RefreshOutcome::Stale) => {
                line = line
                    .child(t_with("library-count", &[("count", &count.to_string())]))
                    .child(t("catalog-refresh-failed"));
            }
            Some(RefreshOutcome::Ok) => {
                line = line
                    .child(t_with("library-count", &[("count", &count.to_string())]))
                    .child(updated);
            }
        }
        line
    }

    /// Nothing in the table: nothing matches the search or the filters.
    fn nothing(&self, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.weak_entity();
        if !page::filtered(&self.registry) {
            return EmptyState::new(Phosphor::Books, t("library-empty-title"))
                .description(t("library-empty-body"))
                .action(Self::install_manually_button(cx))
                .into_any_element();
        }
        EmptyState::new(Phosphor::MagnifyingGlass, t("library-no-match-title"))
            .description(t("no-backends-match-your-search-try-clearing"))
            .action(
                Button::new("clear-filters")
                    .outline()
                    .label(t("library-clear-filters"))
                    .on_click({
                        let this = this.clone();
                        move |_, window, cx| {
                            _ = this.update(cx, |page, cx| page.clear_filters(window, cx));
                        }
                    }),
            )
            .action(Self::install_manually_button(cx))
            .into_any_element()
    }

    fn clear_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        page::clear_filters(&mut self.registry);
        self.search
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.sync_selects(window, cx);
        self.sync(window, cx);
        cx.notify();
    }

    fn install_manually_button(cx: &mut Context<Self>) -> Button {
        let this = cx.weak_entity();
        Button::new("install-manually")
            .outline()
            .icon(Phosphor::Package)
            .label(t("install-manually-progress"))
            .on_click(move |_, window, cx| {
                _ = this.update(cx, |page, cx| page.open_add(window, cx));
            })
    }

    /// The table, or what stands in for it while it is empty.
    fn table(&self, rows: usize, cx: &mut Context<Self>) -> AnyElement {
        let loaded = self.registry.last_refresh.is_some() || !self.library.backends.is_empty();
        if rows == 0 && loaded {
            return div()
                .flex_1()
                .min_h_0()
                .flex()
                .items_center()
                .justify_center()
                .p_6()
                .child(self.nothing(cx))
                .into_any_element();
        }
        v_flex()
            .flex_1()
            .min_h_0()
            .child(
                div().flex_1().min_h_0().child(
                    DataTable::new(&self.table)
                        .bordered(false)
                        .accessibility_label(t("library")),
                ),
            )
            .child(self.summary(rows, cx))
            .into_any_element()
    }

    /// The list a narrow window has in place of the table: Installed, then
    /// Available.
    fn list(&self, rows: &[Row], window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.weak_entity();
        let padding = width::page_padding(window);
        let theme = cx.theme();
        let heading = |key: &str| {
            div()
                .px_2()
                .pt_3()
                .pb_1()
                .text_xs()
                .font_weight(gpui_kit::FontWeight::MEDIUM)
                .text_color(theme.muted_foreground)
                .child(t(key))
        };
        let item = |row: &Row| {
            let source = row.source.clone();
            let kinds = kinds_words(&row.kinds);
            let runs = runs_words(row.runs);
            h_flex()
                .id(SharedString::from(format!("row-{}", row.source)))
                .w_full()
                .gap_2()
                .px_2()
                .py_1p5()
                .rounded(theme.radius)
                .cursor_pointer()
                .hover(|style| style.bg(theme.list_hover))
                .when(
                    self.library.selected.as_deref() == Some(row.source.as_str()),
                    |item| item.bg(theme.list_active),
                )
                .on_click(Self::press(
                    &this,
                    Msg::Library(Input::Select(Some(source))),
                ))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(
                            h_flex()
                                .gap_1p5()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(gpui_kit::FontWeight::MEDIUM)
                                        .truncate()
                                        .child(row.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(row.version.clone()),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .truncate()
                                .child(format!("{kinds} · {runs}")),
                        ),
                )
                .child(state_cell(row, true, cx))
        };
        let installed: Vec<_> = rows.iter().filter(|r| r.installed).map(item).collect();
        let available: Vec<_> = rows.iter().filter(|r| !r.installed).map(item).collect();
        v_flex()
            .id("library-list")
            .size_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .px(padding - px(8.))
                    .pb_2()
                    .when(!installed.is_empty(), |list| {
                        list.child(heading("installed")).children(installed)
                    })
                    .when(!available.is_empty(), |list| {
                        list.child(heading("library-available")).children(available)
                    }),
            )
            .child(
                div()
                    .px(padding - px(12.))
                    .child(self.summary(rows.len(), cx)),
            )
            .into_any_element()
    }
}

/// The State cell of `row`: an install's progress, a status, or a word.
fn state_cell(row: &Row, narrow: bool, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let muted = |words: SharedString| {
        div()
            .text_xs()
            .text_color(theme.muted_foreground)
            .truncate()
            .child(words)
            .into_any_element()
    };
    match &row.state {
        RowState::Available => div().into_any_element(),
        RowState::Installed => {
            if narrow {
                div().into_any_element()
            } else {
                muted(t("installed"))
            }
        }
        RowState::UpdateReady(_) => muted(t("library-update-ready")),
        RowState::NeedsKey => {
            StatusPill::new(Dot::Yellow, t("api-key-required")).into_any_element()
        }
        RowState::Failed(_) => StatusPill::new(Dot::Red, t("failed")).into_any_element(),
        RowState::NeedsNewerApp(_) => muted(t("needs-a-newer-super-voice")),
        RowState::CannotRun(_) => muted(t("library-cannot-run-here")),
        RowState::Installing { phase, fraction } => {
            let bar = Progress::new(SharedString::from(format!("progress-{}", row.source)))
                .xsmall()
                .w(px(if narrow { 48. } else { 72. }))
                .loading(fraction.is_none())
                .value(fraction.unwrap_or(0.) * 100.);
            let words = fraction.map_or_else(
                || phase_words(*phase),
                |f| format!("{}%", (f * 100.).round()).into(),
            );
            h_flex()
                .gap_2()
                .items_center()
                .child(bar)
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(words),
                )
                .into_any_element()
        }
    }
}

/// An install's step, in words.
fn phase_words(phase: InstallPhase) -> SharedString {
    t(match phase {
        InstallPhase::Resolving => "registry-phase-resolving",
        InstallPhase::Downloading => "registry-phase-downloading",
        InstallPhase::Verifying => "registry-phase-verifying",
        InstallPhase::Extracting => "registry-phase-extracting",
        InstallPhase::Installing => "registry-phase-installing",
        InstallPhase::Rescanning => "registry-phase-finishing",
    })
}

/// What a backend's models are for, in words.
fn kinds_words(kinds: &[ModelKind]) -> String {
    kinds
        .iter()
        .map(|kind| kind_words(*kind).to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn kind_words(kind: ModelKind) -> SharedString {
    t(match kind {
        ModelKind::Transcription => "transcription",
        ModelKind::PostProcessing => "post-processing",
        ModelKind::Synthesis => "synthesis",
    })
}

/// The features a backend serves, in words.
fn features_words(features: &[Feature]) -> String {
    features
        .iter()
        .map(|feature| t(feature.key()).to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn runs_words(runs: Runs) -> SharedString {
    match runs {
        Runs::Local => t("local"),
        Runs::Cloud => t("cloud"),
    }
}

impl LibraryPage {
    /// The table with the inspector beside it.
    fn beside(&self, rows: usize, selected: Option<&Row>, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.weak_entity();
        let inspector = match selected {
            Some(row) => v_flex()
                .size_full()
                .child(
                    v_flex()
                        .id("inspector")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .p_4()
                        .gap_3()
                        .child(inspector::heading(row, true, cx))
                        .child(inspector::body(row, false, &this, cx)),
                )
                .child(
                    div()
                        .px_4()
                        .py_3()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .child(inspector::actions(row, &this, cx)),
                )
                .into_any_element(),
            None => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p_4()
                .child(
                    EmptyState::new(Phosphor::Books, t("library-nothing-picked"))
                        .description(t("library-nothing-picked-body")),
                )
                .into_any_element(),
        };
        h_flex()
            .flex_1()
            .min_h_0()
            .items_stretch()
            .child(v_flex().flex_1().min_w_0().child(self.table(rows, cx)))
            .child(
                div()
                    .w(px(INSPECTOR))
                    .flex_shrink_0()
                    .border_l_1()
                    .border_color(cx.theme().border)
                    .child(inspector),
            )
            .into_any_element()
    }

    /// A narrow window's page: the list, or the inspector alone with a back
    /// button.
    fn narrow(
        &self,
        rows: &[Row],
        selected: Option<&Row>,
        toolbar: AnyElement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let this = cx.weak_entity();
        let padding = width::page_padding(window);
        let detail = selected.map(|row| {
            v_flex()
                .size_full()
                .child(
                    v_flex()
                        .id("inspector-page")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .px(padding)
                        .py_3()
                        .gap_3()
                        .child(inspector::heading(row, true, cx))
                        .child(inspector::body(row, true, &this, cx)),
                )
                .child(
                    div()
                        .px(padding)
                        .py_3()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .child(inspector::actions(row, &this, cx)),
                )
        });
        let list = if rows.is_empty()
            && (self.registry.last_refresh.is_some() || !self.library.backends.is_empty())
        {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p_4()
                .child(self.nothing(cx))
                .into_any_element()
        } else {
            self.list(rows, window, cx)
        };
        v_flex()
            .size_full()
            .child(PageHeader::new(Page::Library))
            .when(detail.is_none(), |page| page.child(toolbar))
            .child(div().flex_1().min_h_0().child(
                ListDetail::new("library", list).detail(detail).back(
                    t("library"),
                    Self::press(&this, Msg::Library(Input::Select(None))),
                ),
            ))
            .into_any_element()
    }
}

impl Render for LibraryPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let layout = width::inspector(width::content(window));
        // The columns follow the width; a resize changes them.
        let columns = table::columns(width::content(window));
        if self.table.read(cx).delegate().columns != columns {
            self.sync(window, cx);
        }
        self.sync_selects(window, cx);
        let rows = page::rows(&self.registry, &self.library);
        let selected = page::selected(&self.registry, &self.library);
        let toolbar = self.toolbar(layout, window, cx);
        let body = match layout {
            Inspector::Beside => self.beside(rows.len(), selected.as_ref(), cx),
            Inspector::Sheet => v_flex()
                .flex_1()
                .min_h_0()
                .child(self.table(rows.len(), cx))
                .into_any_element(),
            Inspector::Page => {
                return self.narrow(&rows, selected.as_ref(), toolbar, window, cx);
            }
        };
        v_flex()
            .size_full()
            .child(PageHeader::new(Page::Library).action(Self::install_manually_button(cx)))
            .child(toolbar)
            .child(body)
            .into_any_element()
    }
}

#[cfg(feature = "screenshot")]
mod pretend;

#[cfg(test)]
mod tests {
    /// Every word the page shows is in the English catalog: a missing key
    /// would show as the key itself.
    #[test]
    fn every_word_is_in_the_catalog() {
        for key in [
            "library-all-features",
            "library-all-kinds",
            "api-key-required",
            "backend-version-source",
            "built-for",
            "cancel",
            "catalog-not-loaded",
            "catalog-refresh-failed",
            "catalog-updated",
            "check",
            "checking-progress",
            "choose-a-folder-that-holds-a-backend",
            "choose-backend-folder",
            "choose-progress",
            "cloud",
            "library-configure",
            "contexts",
            "copied-from-disk",
            "failed",
            "failed-detail",
            "feature",
            "from-its-latest-release",
            "git-repository",
            "install-backend",
            "install-backend-progress",
            "install-manually",
            "install-manually-progress",
            "installed",
            "kind",
            "library-available",
            "library-cannot-run-here",
            "library-clear-filters",
            "library-configure-backend",
            "library-count",
            "library-empty-body",
            "library-empty-title",
            "library-filters",
            "library-filters-count",
            "library-folder-path",
            "library-install-failed",
            "library-installed",
            "library-no-match-title",
            "library-nothing-picked",
            "library-nothing-picked-body",
            "library-runs",
            "library-state",
            "library-update-backend",
            "library-update-ready",
            "library-version",
            "license",
            "loading-registry-progress",
            "local",
            "local-folder",
            "library-local-or-cloud",
            "models",
            "library-more",
            "name",
            "native-subprocess",
            "needs",
            "needs-a-newer-super-voice",
            "no-backends-match-your-search-try-clearing",
            "nothing-runs-offline",
            "nothing-to-preview-yet",
            "paste-a-repository-url-and-press-check",
            "post-processing",
            "reaches",
            "reading-source",
            "refresh-catalog",
            "registry-phase-downloading",
            "registry-phase-extracting",
            "registry-phase-finishing",
            "registry-phase-installing",
            "registry-phase-resolving",
            "registry-phase-verifying",
            "registry-unreachable",
            "retry",
            "runs-as",
            "runs-on",
            "search-backends-or-models-progress",
            "show-incompatible",
            "size-gb",
            "size-mb",
            "source",
            "speaks-in-voices-you-clone",
            "speech-to-text",
            "synthesis",
            "text-to-speech",
            "this-backend-cannot-run-on-this-computer",
            "transcription",
            "transmits-audio-to",
            "uninstall",
            "uninstall-backend-body",
            "uninstall-backend-in-use",
            "uninstall-backend-question",
            "uninstall-failed",
            "library-uninstall",
            "update-backend-progress",
            "update-to-version",
            "uses-your-dictation-context",
            "version",
            "voice-cloning",
            "wasm-component-sandboxed",
        ] {
            assert!(
                super_voice_app_core::i18n::LANGUAGE_LOADER.has(key),
                "no message {key}"
            );
        }
    }
}
