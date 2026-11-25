// SPDX-License-Identifier: GPL-3.0-only

//! Models-page UI selection/menu state, extracted from `AppModel` following the
//! `RegistryState` template (App Tier 3 #15).

use cosmic::widget::segmented_button;

use crate::state::ModelsTab;

/// Ephemeral Models-page UI state: the Installed/Browse tab bar plus the
/// active-backend card's selection, staging, and menu flags.
pub struct ModelsPageState {
    /// Installed / Download tab bar (the active tab carries a [`ModelsTab`]).
    pub models_tabs: segmented_button::SingleSelectModel,
    /// `source` of the installed-backend card whose overflow ("⋯") menu is
    /// open, if any. Only one is open at a time.
    pub installed_menu_open: Option<String>,
    /// Whether the Browse toolbar's overflow ("⋯") menu is open. It holds
    /// Refresh and the manual-install drawer, which used to be two buttons
    /// competing with the search field.
    pub browse_menu_open: bool,
    /// The pipeline step whose card the Models page shows under the rows.
    pub pipeline_step: crate::state::Stage,
}

impl Default for ModelsPageState {
    fn default() -> Self {
        let mut models_tabs = segmented_button::SingleSelectModel::default();
        models_tabs
            .insert()
            .text(crate::fl!("installed"))
            .data(ModelsTab::Installed)
            .activate();
        models_tabs
            .insert()
            .text(crate::fl!("browse"))
            .data(ModelsTab::Download);
        Self {
            models_tabs,
            installed_menu_open: None,
            browse_menu_open: false,
            pipeline_step: crate::state::Stage::Transcription,
        }
    }
}
