//! The confirmation engine: the mandatory preview then confirm/edit/cancel
//! sequence from docs/05-confirmation-workflow.md.
//!
//! This is the single enforcement point for the rule the whole product exists
//! to guarantee: no write reaches an adapter's execute without a successful
//! preview for that same intent having run first. Built in T8.
