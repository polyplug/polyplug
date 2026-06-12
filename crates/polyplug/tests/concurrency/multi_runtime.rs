#![allow(clippy::expect_used)]

//! Multi-runtime parallelism — N runtimes built, used, and destroyed across
//! threads simultaneously, asserting Rule-12 isolation and no leak.
//!
//! (Populated by task #47c.)
