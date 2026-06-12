#![allow(clippy::expect_used)]

//! Concurrent `host->log` delivery from many threads into one `LoggerHandle`
//! funnel — no lost or torn records, no deadlock.
//!
//! (Populated by task #47d.)
