// SPDX-License-Identifier: GPL-3.0-only
//! `/assistants`' work: connecting and disconnecting an assistant, the keys
//! made by hand, and what each assistant is told about asking aloud. What the
//! assistants are and how Super Voice gets into their configs is
//! [`crate::assistants`].

use crate::assistants::calls::Caller;
use crate::assistants::{self, Error, KEY_SCOPES, MCP_PATH};
use crate::daemon::types::SuperSTTDaemon;
use chrono::SecondsFormat;
use log::{info, warn};
use super_engine_daemon::auth::{AssistantKey, Auth};
use super_voice_shared::models::assistants::{
    Assistant, AssistantId, AssistantList, CustomKey, NewKey,
};

/// The key the daemon made for `id`, if it holds one. It names an
/// assistant's key by the assistant's wire id.
fn key_of(keys: &[AssistantKey], id: AssistantId) -> Option<&AssistantKey> {
    keys.iter().find(|key| key.name == id.as_wire_str())
}

/// Whether `name` is an assistant's, whose key the daemon names itself.
fn is_assistant_name(name: &str) -> bool {
    name.parse::<AssistantId>().is_ok()
}

fn key_scopes() -> Vec<String> {
    KEY_SCOPES
        .iter()
        .map(|scope| (*scope).to_string())
        .collect()
}

impl SuperSTTDaemon {
    /// Where an MCP client reaches Super Voice: the MCP server on the
    /// daemon's TCP listener.
    pub async fn mcp_address(&self) -> String {
        let port = self.config.read().await.http.tcp.port;
        format!("http://127.0.0.1:{port}{MCP_PATH}")
    }

    /// Every assistant and where it stands, every key made by hand, and the
    /// address.
    pub async fn list_assistants(&self, auth: &Auth) -> AssistantList {
        let keys = auth.assistant_keys();
        let has_key: Vec<(AssistantId, bool)> = AssistantId::ALL
            .iter()
            .map(|id| (*id, key_of(&keys, *id).is_some()))
            .collect();
        // Reading the configs and looking for the programs is file I/O.
        let states = tokio::task::spawn_blocking(move || {
            has_key
                .into_iter()
                .map(|(id, has_key)| (id, assistants::state(id, has_key)))
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_else(|e| {
            warn!("Could not look for the assistants: {e}");
            Vec::new()
        });
        let config = self.config.read().await;
        let assistants = states
            .into_iter()
            .map(|(id, state)| Assistant {
                id,
                state,
                announce_when_needed: config.assistants.announces_when_needed(id),
            })
            .collect();
        drop(config);
        let custom_keys = keys
            .iter()
            .filter(|key| !is_assistant_name(&key.name))
            .map(|key| CustomKey {
                id: key.id.clone(),
                name: key.name.clone(),
                created_at: key.created_at.to_rfc3339_opts(SecondsFormat::Secs, true),
            })
            .collect();
        AssistantList {
            assistants,
            custom_keys,
            address: self.mcp_address().await,
        }
    }

    /// Connect assistant `id`: make it a key, or use the one it has, and
    /// write Super Voice into its config. Connecting one that is connected
    /// already writes its entry again, which changes nothing.
    ///
    /// # Errors
    ///
    /// See [`Error`]. A key made for this connect is revoked again when the
    /// rest fails.
    pub async fn connect_assistant(
        &self,
        auth: &Auth,
        id: AssistantId,
    ) -> Result<AssistantList, Error> {
        let _changing = self.assistants.changing().await;
        let found = tokio::task::spawn_blocking(move || assistants::is_found(id))
            .await
            .unwrap_or(false);
        if !found {
            return Err(Error::NotFound);
        }
        let keys = auth.assistant_keys();
        let held = key_of(&keys, id)
            .and_then(|key| Some((key.id.clone(), auth.assistant_key_secret(&key.id)?)));
        let (key_id, secret, minted) = if let Some((key_id, secret)) = held {
            (key_id, secret, false)
        } else {
            let (secret, key) = auth
                .mint_assistant_key(id.as_wire_str(), &key_scopes())
                .await
                .map_err(|e| Error::Key(e.to_string()))?;
            info!("Made a key for {}: {}", id.display_name(), key.id);
            (key.id, secret, true)
        };
        if let Err(e) = self.write_assistant_entry(id, &secret).await {
            if minted {
                self.revoke_keys(auth, &[key_id]).await;
            }
            return Err(e);
        }
        Ok(self.list_assistants(auth).await)
    }

    /// Put Super Voice in the config of `id`, with `secret`.
    async fn write_assistant_entry(&self, id: AssistantId, secret: &str) -> Result<(), Error> {
        if id == AssistantId::ClaudeCode {
            let claude = tokio::task::spawn_blocking(move || assistants::find_program(id))
                .await
                .ok()
                .flatten()
                .ok_or(Error::NotFound)?;
            return assistants::install_claude_plugin(&claude).await;
        }
        let url = self.mcp_address().await;
        let secret = secret.to_string();
        tokio::task::spawn_blocking(move || assistants::write_entry(id, &url, &secret))
            .await
            .unwrap_or_else(|e| Err(Error::Config(e.to_string())))
    }

    /// Disconnect assistant `id`: take Super Voice out of its config, revoke
    /// its key, and cancel its calls in flight. Disconnecting one that is not
    /// connected changes nothing.
    ///
    /// The key is revoked even when the config cannot be changed: without it
    /// the entry left behind does nothing.
    ///
    /// # Errors
    ///
    /// See [`Error`].
    pub async fn disconnect_assistant(
        &self,
        auth: &Auth,
        id: AssistantId,
    ) -> Result<AssistantList, Error> {
        let _changing = self.assistants.changing().await;
        let removed = if id == AssistantId::ClaudeCode {
            let claude = tokio::task::spawn_blocking(move || assistants::find_program(id))
                .await
                .ok()
                .flatten();
            match claude {
                Some(claude) => assistants::uninstall_claude_plugin(&claude).await,
                // Without Claude Code there is nothing to uninstall it from.
                None => Ok(()),
            }
        } else {
            tokio::task::spawn_blocking(move || assistants::remove_entry(id))
                .await
                .unwrap_or_else(|e| Err(Error::Config(e.to_string())))
        };
        let keys: Vec<String> = auth
            .assistant_keys()
            .into_iter()
            .filter(|key| key.name == id.as_wire_str())
            .map(|key| key.id)
            .collect();
        self.revoke_keys(auth, &keys).await;
        removed?;
        Ok(self.list_assistants(auth).await)
    }

    /// Revoke each of `keys`, and cancel its calls in flight.
    async fn revoke_keys(&self, auth: &Auth, keys: &[String]) {
        for key in keys {
            match auth.revoke_assistant_key(key).await {
                Ok(_) => info!("Revoked key {key}"),
                Err(e) => warn!("Could not revoke key {key}: {e}"),
            }
            let cancelled = self
                .assistants
                .calls
                .cancel_caller(&Caller::Key(key.clone()));
            if cancelled > 0 {
                info!("Cancelled {cancelled} calls made with key {key}");
            }
        }
    }

    /// Keep whether Super Voice says aloud when assistant `id` needs the user.
    /// The next announcement reads it.
    pub async fn set_assistant_announces(
        &self,
        auth: &Auth,
        id: AssistantId,
        on: bool,
    ) -> AssistantList {
        if let Err(e) = self
            .set_config_field(|c| c.assistants.set_announces_when_needed(id, on))
            .await
        {
            warn!("Announcing changed but config save failed: {e}");
        }
        self.list_assistants(auth).await
    }

    /// Make a key by hand, named `name`, for an MCP client that is not one
    /// of the assistants. The key itself is in the answer, and nowhere else
    /// later.
    ///
    /// # Errors
    ///
    /// [`Error::NameRequired`] for a blank name, [`Error::NameReserved`] for
    /// an assistant's, [`Error::Key`] when the secrets file cannot be written.
    pub async fn create_assistant_key(&self, auth: &Auth, name: &str) -> Result<NewKey, Error> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::NameRequired);
        }
        if is_assistant_name(name) {
            return Err(Error::NameReserved);
        }
        let (secret, key) = auth
            .mint_assistant_key(name, &key_scopes())
            .await
            .map_err(|e| Error::Key(e.to_string()))?;
        info!("Made key {} for {name}", key.id);
        Ok(NewKey {
            id: key.id,
            name: key.name,
            key: secret,
            address: self.mcp_address().await,
        })
    }

    /// Revoke the key made by hand with id `key_id`, and cancel its calls in
    /// flight. An assistant's key is not one: disconnecting the assistant
    /// revokes it.
    ///
    /// # Errors
    ///
    /// [`Error::UnknownKey`] when no key made by hand has that id.
    pub async fn revoke_custom_key(
        &self,
        auth: &Auth,
        key_id: &str,
    ) -> Result<AssistantList, Error> {
        let known = auth
            .assistant_keys()
            .into_iter()
            .any(|key| key.id == key_id && !is_assistant_name(&key.name));
        if !known {
            return Err(Error::UnknownKey);
        }
        self.revoke_keys(auth, &[key_id.to_string()]).await;
        Ok(self.list_assistants(auth).await)
    }

    /// The key assistant `id` sends, or `None` when it has none.
    #[must_use]
    pub fn assistant_key(&self, auth: &Auth, id: AssistantId) -> Option<String> {
        key_of(&auth.assistant_keys(), id).and_then(|key| auth.assistant_key_secret(&key.id))
    }
}
