// SPDX-License-Identifier: GPL-3.0-only

use crate::ui::messages::Message;
use cosmic::prelude::*;

use super::AppModel;

impl AppModel {
    /// Updates the header and window titles.
    pub(in crate::core::app) fn update_title(&mut self) -> Task<cosmic::Action<Message>> {
        let mut window_title = "Super Voice".to_string();

        if let Some(page) = self.nav.text(self.nav.active()) {
            window_title.push_str(" — ");
            window_title.push_str(page);
        }

        if let Some(id) = self.core.main_window_id() {
            self.set_window_title(window_title, id)
        } else {
            Task::none()
        }
    }
}
