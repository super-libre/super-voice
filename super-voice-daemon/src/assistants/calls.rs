// SPDX-License-Identifier: GPL-3.0-only
//! The tool calls in flight on `/v1/mcp`, so each can be cancelled: by the
//! client that made it, with `notifications/cancelled`, by its connection
//! closing, or by the user disconnecting the assistant or revoking its key.
//!
//! Revoking a key stops its next request, not one already running. A
//! `listen` can wait minutes for the user, so the daemon cancels the key's
//! calls itself when it revokes it.

use parking_lot::Mutex;
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio_util::sync::CancellationToken;

/// Who made a call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Caller {
    /// An assistant, by the id of its key.
    Key(String),
    /// A client with a session, such as a test or the CLI over the Unix
    /// socket, by its token.
    Session(String),
}

/// Every tool call in flight.
#[derive(Default)]
pub struct Calls {
    next: AtomicU64,
    entries: Mutex<Vec<Entry>>,
}

struct Entry {
    serial: u64,
    caller: Caller,
    /// The JSON-RPC id, as JSON text, so `1` and `"1"` stay two ids.
    request: String,
    cancel: CancellationToken,
}

/// One call in flight. Dropped when the call ends, which forgets it.
#[must_use = "dropping it forgets the call at once"]
pub struct InFlight {
    calls: Arc<Calls>,
    serial: u64,
    cancel: CancellationToken,
}

impl InFlight {
    /// Cancelled when the call is.
    #[must_use]
    pub fn token(&self) -> CancellationToken {
        self.cancel.clone()
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.calls
            .entries
            .lock()
            .retain(|entry| entry.serial != self.serial);
    }
}

impl Calls {
    /// Keep a call `caller` made with JSON-RPC id `request`, until the
    /// returned guard is dropped.
    ///
    /// Two calls can share an id: the server is stateless, so two sessions
    /// of one assistant both count from 1. Both are kept, and a cancel names
    /// both.
    pub fn begin(self: &Arc<Self>, caller: Caller, request: &Value) -> InFlight {
        let serial = self.next.fetch_add(1, Ordering::Relaxed);
        let cancel = CancellationToken::new();
        self.entries.lock().push(Entry {
            serial,
            caller,
            request: request.to_string(),
            cancel: cancel.clone(),
        });
        InFlight {
            calls: Arc::clone(self),
            serial,
            cancel,
        }
    }

    /// Cancel the calls `caller` made with JSON-RPC id `request`. How many
    /// were.
    pub fn cancel(&self, caller: &Caller, request: &Value) -> usize {
        let request = request.to_string();
        self.cancel_where(|entry| entry.caller == *caller && entry.request == request)
    }

    /// Cancel every call `caller` made. How many there were.
    pub fn cancel_caller(&self, caller: &Caller) -> usize {
        self.cancel_where(|entry| entry.caller == *caller)
    }

    /// How many calls are in flight.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.lock().len()
    }

    /// Whether no call is in flight.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn cancel_where(&self, matches: impl Fn(&Entry) -> bool) -> usize {
        let entries = self.entries.lock();
        let mut cancelled = 0;
        for entry in entries.iter().filter(|entry| matches(entry)) {
            entry.cancel.cancel();
            cancelled += 1;
        }
        cancelled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn key(id: &str) -> Caller {
        Caller::Key(id.to_string())
    }

    #[test]
    fn a_cancel_reaches_only_the_call_it_names() {
        let calls = Arc::new(Calls::default());
        let mine = calls.begin(key("a"), &json!(1));
        let other_id = calls.begin(key("a"), &json!(2));
        let same_text = calls.begin(key("a"), &json!("1"));
        let other_caller = calls.begin(key("b"), &json!(1));

        assert_eq!(calls.cancel(&key("a"), &json!(1)), 1);
        assert!(mine.token().is_cancelled());
        assert!(!other_id.token().is_cancelled());
        assert!(!same_text.token().is_cancelled(), "1 and \"1\" are two ids");
        assert!(!other_caller.token().is_cancelled());
    }

    /// Two sessions of one assistant count from 1 alike, and the server
    /// cannot tell them apart, so a cancel reaches both.
    #[test]
    fn calls_sharing_an_id_are_both_kept_and_both_cancelled() {
        let calls = Arc::new(Calls::default());
        let first = calls.begin(key("a"), &json!(3));
        let second = calls.begin(key("a"), &json!(3));
        assert_eq!(calls.len(), 2);
        assert_eq!(calls.cancel(&key("a"), &json!(3)), 2);
        assert!(first.token().is_cancelled() && second.token().is_cancelled());
    }

    /// Revoking a key cancels every call it has running.
    #[test]
    fn cancelling_a_caller_cancels_all_its_calls() {
        let calls = Arc::new(Calls::default());
        let one = calls.begin(key("a"), &json!(1));
        let two = calls.begin(key("a"), &json!(2));
        let session = calls.begin(Caller::Session("t".to_string()), &json!(1));
        assert_eq!(calls.cancel_caller(&key("a")), 2);
        assert!(one.token().is_cancelled() && two.token().is_cancelled());
        assert!(!session.token().is_cancelled());
    }

    #[test]
    fn a_call_is_forgotten_when_it_ends() {
        let calls = Arc::new(Calls::default());
        let call = calls.begin(key("a"), &json!(1));
        assert!(!calls.is_empty());
        drop(call);
        assert!(calls.is_empty());
        assert_eq!(calls.cancel(&key("a"), &json!(1)), 0);
    }
}
