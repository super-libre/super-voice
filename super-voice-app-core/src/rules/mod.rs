// SPDX-License-Identifier: GPL-3.0-only
//! The rules that decide what a page shows, kept apart from how any app draws
//! it. Each returns a value, not words: every app says it its own way.

pub mod backends;
pub mod memory;
pub mod status;
