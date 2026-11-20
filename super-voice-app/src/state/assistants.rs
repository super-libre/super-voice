// SPDX-License-Identifier: GPL-3.0-only
//! The Assistants page: what the daemon says about each AI assistant, and the
//! two dialogs the page opens.

use std::collections::HashSet;

use super_voice_shared::models::assistants::{AssistantId, AssistantList, NewKey};

/// What the Assistants page shows.
#[derive(Debug, Clone, Default)]
pub struct AssistantsState {
    /// The daemon's last answer. `None` until the page is first shown.
    pub list: Option<AssistantList>,
    /// The Connect dialog, while it is open.
    pub connect: Option<ConnectDialog>,
    /// The dialog that makes a key for another assistant, while it is open.
    pub key: Option<KeyDialog>,
    /// The assistants with a Disconnect running, whose button waits for it.
    pub disconnecting: HashSet<AssistantId>,
    /// The assistants connected since the app started. An assistant reads its
    /// config when it starts, so each row says to restart it.
    pub connected_here: HashSet<AssistantId>,
}

impl AssistantsState {
    /// Whether a dialog is over the page, which then shows the page's error.
    #[must_use]
    pub fn dialog_open(&self) -> bool {
        self.connect.is_some() || self.key.is_some()
    }
}

/// The Connect dialog: the assistant it asks about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectDialog {
    pub id: AssistantId,
    /// Whether the user said Connect and the daemon has not answered yet.
    pub connecting: bool,
}

/// The dialog that makes a key for another assistant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyDialog {
    /// Asking for the key's name.
    Naming {
        name: String,
        /// Whether the user said Create and the daemon has not answered yet.
        creating: bool,
    },
    /// The key the daemon made. It is shown only now.
    Made(NewKey),
}

impl KeyDialog {
    /// The dialog as it opens: an empty name.
    #[must_use]
    pub fn new() -> Self {
        Self::Naming {
            name: String::new(),
            creating: false,
        }
    }

    /// The name to make the key under, once there is one to send: not blank,
    /// and not already sent.
    #[must_use]
    pub fn name_to_create(&self) -> Option<&str> {
        match self {
            Self::Naming {
                name,
                creating: false,
            } if !name.trim().is_empty() => Some(name.trim()),
            _ => None,
        }
    }
}

impl Default for KeyDialog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::KeyDialog;

    /// Create waits for a name, and is sent once.
    #[test]
    fn a_key_is_made_once_and_only_with_a_name() {
        assert_eq!(KeyDialog::new().name_to_create(), None);
        let blank = KeyDialog::Naming {
            name: "   ".to_string(),
            creating: false,
        };
        assert_eq!(blank.name_to_create(), None);
        let named = KeyDialog::Naming {
            name: " Zed ".to_string(),
            creating: false,
        };
        assert_eq!(named.name_to_create(), Some("Zed"));
        let sent = KeyDialog::Naming {
            name: "Zed".to_string(),
            creating: true,
        };
        assert_eq!(sent.name_to_create(), None);
    }
}
