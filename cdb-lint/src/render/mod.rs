//! Turning a [`ConformanceReport`] into an artifact somebody can read.
//!
//! One module per `--format`. Every one of them owes design §5's honesty
//! rules the same thing in its own vocabulary: the three
//! [`ContentCoverage`] states must survive the translation, because a format
//! that flattens them reports "we did not look" as "we looked and it was
//! fine" — the one lie the third state exists to prevent.
//!
//! [`ConformanceReport`]: rusty_cdb::conformance::ConformanceReport
//! [`ContentCoverage`]: rusty_cdb::conformance::ContentCoverage

pub mod text;
