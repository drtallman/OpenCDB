//! RFC 3339 datetimes and temporal intervals.
//!
//! Implements Requirement Metadata6 (`/req/core/metadata-datetime`, §7.9.3.6):
//! date and time SHALL be in UTC and formatted per RFC 3339 §5.6 — and
//! Requirement Metadata7 (`/req/core/metadata-temporal-interval`, §7.9.3.7):
//! a temporal geometry is either a datetime or an interval, where intervals
//! may be half-bounded using `..` or an empty string.

use std::fmt;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::MetadataViolation;

/// Parses an RFC 3339 datetime, enforcing UTC (Metadata6-A). `Z` and
/// `+00:00` are accepted; any non-zero offset is rejected.
///
/// Strict on the `T` separator: chrono tolerates a space, but the RFC 3339
/// ABNF requires `T`, and CDB's determinism principle favors one form.
pub fn parse_datetime(value: &str) -> Result<DateTime<Utc>, MetadataViolation> {
    if value.contains(' ') {
        return Err(MetadataViolation::InvalidDateTime {
            value: value.to_owned(),
            reason: "RFC 3339 requires the `T` date/time separator".to_owned(),
        });
    }
    let parsed =
        DateTime::parse_from_rfc3339(value).map_err(|e| MetadataViolation::InvalidDateTime {
            value: value.to_owned(),
            reason: e.to_string(),
        })?;
    if parsed.offset().local_minus_utc() != 0 {
        return Err(MetadataViolation::NotUtc {
            value: value.to_owned(),
        });
    }
    Ok(parsed.with_timezone(&Utc))
}

/// Formats a datetime in canonical RFC 3339 UTC form (`Z` suffix).
pub fn format_datetime(value: &DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

/// A temporal geometry per the Metadata7 ABNF: an instant, or an interval
/// with at least one bound. The ABNF has no fully unbounded `../..` form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Temporal {
    Instant(DateTime<Utc>),
    Interval {
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
    },
}

impl Temporal {
    pub fn parse(value: &str) -> Result<Temporal, MetadataViolation> {
        let invalid = |reason: &'static str| MetadataViolation::InvalidTemporalInterval {
            value: value.to_owned(),
            reason,
        };
        let parts: Vec<&str> = value.split('/').collect();
        match parts.as_slice() {
            [instant] => Ok(Temporal::Instant(parse_datetime(instant)?)),
            [start, end] => {
                let bound = |text: &str| -> Result<Option<DateTime<Utc>>, MetadataViolation> {
                    if text.is_empty() || text == ".." {
                        Ok(None)
                    } else {
                        parse_datetime(text).map(Some)
                    }
                };
                let (start, end) = (bound(start)?, bound(end)?);
                if start.is_none() && end.is_none() {
                    return Err(invalid("an interval requires at least one bound"));
                }
                // Consistent with the OGC API Records datetime semantics the
                // requirement is copied from: start precedes or equals end.
                if let (Some(start), Some(end)) = (&start, &end)
                    && start > end
                {
                    return Err(invalid("interval start must not be after its end"));
                }
                Ok(Temporal::Interval { start, end })
            }
            _ => Err(invalid("expected `datetime` or `start/end`")),
        }
    }
}

impl fmt::Display for Temporal {
    /// Canonical form: half-open bounds render as `..`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Temporal::Instant(instant) => f.write_str(&format_datetime(instant)),
            Temporal::Interval { start, end } => {
                let bound = |b: &Option<DateTime<Utc>>| {
                    b.as_ref().map_or_else(|| "..".to_owned(), format_datetime)
                };
                write!(f, "{}/{}", bound(start), bound(end))
            }
        }
    }
}

impl Serialize for Temporal {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Temporal {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Temporal::parse(&value).map_err(serde::de::Error::custom)
    }
}

/// Serde adapter enforcing Metadata6 on (de)serialization of a datetime.
pub mod rfc3339_utc {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        value: &DateTime<Utc>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&super::format_datetime(value))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<DateTime<Utc>, D::Error> {
        let text = String::deserialize(deserializer)?;
        super::parse_datetime(&text).map_err(serde::de::Error::custom)
    }
}

/// [`rfc3339_utc`] for optional fields.
pub mod rfc3339_utc_option {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        value: &Option<DateTime<Utc>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(instant) => serializer.serialize_str(&super::format_datetime(instant)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<DateTime<Utc>>, D::Error> {
        Option::<String>::deserialize(deserializer)?
            .map(|text| super::parse_datetime(&text).map_err(serde::de::Error::custom))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn dt(text: &str) -> DateTime<Utc> {
        parse_datetime(text).unwrap()
    }

    /// §7.9.3.6 Requirement Metadata6 — UTC datetimes accepted.
    #[test]
    fn req_core_metadata_datetime_accepts_utc_forms() {
        assert_eq!(
            dt("2026-07-14T09:30:00Z"),
            Utc.with_ymd_and_hms(2026, 7, 14, 9, 30, 0).unwrap()
        );
        assert_eq!(dt("2026-07-14T09:30:00+00:00"), dt("2026-07-14T09:30:00Z"));
        assert_eq!(
            dt("2026-07-14T09:30:00.250Z").timestamp_subsec_millis(),
            250
        );
    }

    /// §7.9.3.6 Metadata6-A — non-UTC offsets are rejected.
    #[test]
    fn req_core_metadata_datetime_rejects_non_utc() {
        for value in ["2026-07-14T09:30:00+02:00", "2026-07-14T09:30:00-05:00"] {
            assert!(
                matches!(parse_datetime(value), Err(MetadataViolation::NotUtc { .. })),
                "{value}"
            );
        }
    }

    /// §7.9.3.6 Metadata6-B — RFC 3339 syntax is required.
    #[test]
    fn req_core_metadata_datetime_rejects_malformed() {
        for value in [
            "",
            "2026-07-14",
            "2026-07-14T09:30:00",
            "2026-07-14 09:30:00Z",
            "14/07/2026",
        ] {
            assert!(
                matches!(
                    parse_datetime(value),
                    Err(MetadataViolation::InvalidDateTime { .. })
                ),
                "{value}"
            );
        }
    }

    #[test]
    fn req_core_metadata_datetime_formats_canonical_z() {
        let instant = Utc.with_ymd_and_hms(2026, 7, 14, 9, 30, 0).unwrap();
        assert_eq!(format_datetime(&instant), "2026-07-14T09:30:00Z");
        assert_eq!(dt(&format_datetime(&instant)), instant);
    }

    /// §7.9.3.7 Requirement Metadata7 — a bare datetime is an instant.
    #[test]
    fn req_core_metadata_temporal_instant() {
        assert_eq!(
            Temporal::parse("2026-07-14T09:30:00Z").unwrap(),
            Temporal::Instant(dt("2026-07-14T09:30:00Z"))
        );
    }

    /// §7.9.3.7 — bounded interval.
    #[test]
    fn req_core_metadata_temporal_bounded_interval() {
        assert_eq!(
            Temporal::parse("2025-01-01T00:00:00Z/2026-01-01T00:00:00Z").unwrap(),
            Temporal::Interval {
                start: Some(dt("2025-01-01T00:00:00Z")),
                end: Some(dt("2026-01-01T00:00:00Z")),
            }
        );
    }

    /// §7.9.3.7 — half-bounded via `..` or the empty string.
    #[test]
    fn req_core_metadata_temporal_half_bounded() {
        let open_start = Temporal::Interval {
            start: None,
            end: Some(dt("2026-01-01T00:00:00Z")),
        };
        assert_eq!(
            Temporal::parse("../2026-01-01T00:00:00Z").unwrap(),
            open_start
        );
        assert_eq!(
            Temporal::parse("/2026-01-01T00:00:00Z").unwrap(),
            open_start
        );

        let open_end = Temporal::Interval {
            start: Some(dt("2025-01-01T00:00:00Z")),
            end: None,
        };
        assert_eq!(
            Temporal::parse("2025-01-01T00:00:00Z/..").unwrap(),
            open_end
        );
        assert_eq!(Temporal::parse("2025-01-01T00:00:00Z/").unwrap(), open_end);
    }

    /// §7.9.3.7 — forms outside the ABNF are rejected.
    #[test]
    fn req_core_metadata_temporal_rejects_invalid() {
        for value in [
            "../..",
            "/",
            "2025-01-01T00:00:00Z/2026-01-01T00:00:00Z/..",
            "2026-01-01T00:00:00Z/2025-01-01T00:00:00Z",
        ] {
            assert!(
                matches!(
                    Temporal::parse(value),
                    Err(MetadataViolation::InvalidTemporalInterval { .. })
                ),
                "{value}"
            );
        }
        // Bounds themselves must satisfy Metadata6.
        assert!(matches!(
            Temporal::parse("2025-01-01T00:00:00+01:00/.."),
            Err(MetadataViolation::NotUtc { .. })
        ));
    }

    #[test]
    fn temporal_display_canonicalizes_and_roundtrips() {
        let parsed = Temporal::parse("/2026-01-01T00:00:00Z").unwrap();
        assert_eq!(parsed.to_string(), "../2026-01-01T00:00:00Z");
        assert_eq!(Temporal::parse(&parsed.to_string()).unwrap(), parsed);

        let json = serde_json::to_string(&parsed).unwrap();
        assert_eq!(json, r#""../2026-01-01T00:00:00Z""#);
        let back: Temporal = serde_json::from_str(&json).unwrap();
        assert_eq!(back, parsed);
    }
}
