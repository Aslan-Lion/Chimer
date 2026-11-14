//! Reserved for the history and analytics store.
//!
//! Not yet implemented: the running timer and session documents are persisted by
//! their own engines, and the library, settings and display configuration are
//! held as JSON documents by the frontend stores. This module holds the name and
//! location a future SQLite database will use when History & Analytics is built.

#![allow(dead_code)]

/// File name of the local SQLite database.
pub const DATABASE_FILE: &str = "app_timer.sqlite3";
