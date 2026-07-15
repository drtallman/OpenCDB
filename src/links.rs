//! Links requirements module.
//!
//! Implements the `/req/core/links` requirements class (spec §7.7): a link
//! describes a relationship with another resource, internal or external.
//! `href` and `rel` are mandatory; `type` (media type hint) and `title` are
//! recommended (Link3/Link4) and therefore optional.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A violation of a SHALL requirement in the links module.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LinkViolation {
    #[error("link href {href:?} is not a valid URL ({reason}) (violates /req/core/link-href)")]
    InvalidHref { href: String, reason: &'static str },
    #[error("link rel is mandatory and must be non-empty (violates /req/core/link-rel)")]
    MissingRel,
}

/// Whether an href is an absolute URL or a relative reference; both are
/// allowed by Requirement Link1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HrefKind {
    Absolute,
    Relative,
}

/// Characters RFC 3986 excludes from URLs; they must be percent-encoded.
const DISALLOWED_URL_CHARACTERS: [char; 9] = ['<', '>', '"', '{', '}', '|', '\\', '^', '`'];

/// Validates an href per Requirement Link1 and classifies it.
pub fn classify_href(href: &str) -> Result<HrefKind, LinkViolation> {
    let invalid = |reason: &'static str| LinkViolation::InvalidHref {
        href: href.to_owned(),
        reason,
    };
    if href.is_empty() {
        return Err(invalid("empty"));
    }
    for character in href.chars() {
        if character.is_whitespace() || character.is_control() {
            return Err(invalid(
                "whitespace and control characters must be percent-encoded",
            ));
        }
        if DISALLOWED_URL_CHARACTERS.contains(&character) {
            return Err(invalid(
                "character excluded by RFC 3986 must be percent-encoded",
            ));
        }
    }
    if let Some((scheme, rest)) = href.split_once(':')
        && is_valid_scheme(scheme)
    {
        return if rest.is_empty() {
            Err(invalid("nothing after URL scheme"))
        } else {
            Ok(HrefKind::Absolute)
        };
    }
    Ok(HrefKind::Relative)
}

/// RFC 3986 §3.1: `ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )`.
fn is_valid_scheme(scheme: &str) -> bool {
    let mut chars = scheme.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// A relationship with another entity (spec §7.7 table).
///
/// Serializes with the spec's field names; the media type hint is `type` on
/// the wire. Construction through [`Link::new`] validates; values built by
/// deserialization should be checked with [`Link::validate`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    /// The actual link, as a URL; relative and absolute both allowed (Link1).
    pub href: String,
    /// The type or semantics of the relation (Link2, mandatory).
    pub rel: String,
    /// Hint of the media type of the referenced entity (Link3, recommended).
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    /// Human readable title for rendered displays (Link4, recommended).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl Link {
    pub fn new(href: impl Into<String>, rel: impl Into<String>) -> Result<Self, LinkViolation> {
        let link = Self {
            href: href.into(),
            rel: rel.into(),
            media_type: None,
            title: None,
        };
        link.validate()?;
        Ok(link)
    }

    #[must_use]
    pub fn with_media_type(mut self, media_type: impl Into<String>) -> Self {
        self.media_type = Some(media_type.into());
        self
    }

    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Checks the SHALL requirements Link1 and Link2.
    pub fn validate(&self) -> Result<(), LinkViolation> {
        classify_href(&self.href)?;
        if self.rel.is_empty() {
            return Err(LinkViolation::MissingRel);
        }
        Ok(())
    }

    pub fn kind(&self) -> Result<HrefKind, LinkViolation> {
        classify_href(&self.href)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §7.7 Requirement Link1 — relative and absolute URLs both allowed.
    #[test]
    fn req_core_link_href_accepts_absolute_and_relative() {
        assert_eq!(
            classify_href("http://data.example.com/buildings/123"),
            Ok(HrefKind::Absolute)
        );
        assert_eq!(
            classify_href("https://example.com/x"),
            Ok(HrefKind::Absolute)
        );
        assert_eq!(
            classify_href("mailto:info@example.com"),
            Ok(HrefKind::Absolute)
        );
        assert_eq!(
            classify_href("../global_metadata/crs.xml"),
            Ok(HrefKind::Relative)
        );
        assert_eq!(
            classify_href("Tiles/RoadNetwork.gpkg"),
            Ok(HrefKind::Relative)
        );
    }

    /// §7.7 Requirement Link1 — malformed URLs are rejected.
    #[test]
    fn req_core_link_href_rejects_malformed() {
        for href in ["", "ht tp://example.com", "a<b", "http:", "line\nbreak"] {
            assert!(
                matches!(classify_href(href), Err(LinkViolation::InvalidHref { .. })),
                "{href:?}"
            );
        }
    }

    /// §7.7 Requirement Link2 — `rel` is mandatory.
    #[test]
    fn req_core_link_rel_is_mandatory() {
        assert_eq!(
            Link::new("http://example.com/x", ""),
            Err(LinkViolation::MissingRel)
        );
        let json = r#"{"href":"http://example.com/x"}"#;
        assert!(serde_json::from_str::<Link>(json).is_err());
    }

    /// §7.7 Recommendations Link3/Link4 — `type` and `title` are optional
    /// and round-trip; the media type hint serializes as `type`.
    #[test]
    fn rec_core_link_media_type_and_title_roundtrip() {
        let link = Link::new("http://data.example.com/buildings/123", "alternate")
            .unwrap()
            .with_media_type("application/geo+json")
            .with_title("Trierer Strasse 70, 53115 Bonn");
        let json = serde_json::to_string(&link).unwrap();
        assert!(json.contains(r#""type":"application/geo+json""#));
        let back: Link = serde_json::from_str(&json).unwrap();
        assert_eq!(back, link);
        assert!(back.validate().is_ok());
    }

    #[test]
    fn rec_core_link_optionals_omitted_when_absent() {
        let link = Link::new("Tiles/RoadNetwork.gpkg", "item").unwrap();
        let value = serde_json::to_value(&link).unwrap();
        let object = value.as_object().unwrap();
        assert_eq!(object.len(), 2);
        assert!(object.contains_key("href") && object.contains_key("rel"));
    }

    #[test]
    fn link_violation_converts_to_cdb_error() {
        let e: crate::CdbError = LinkViolation::MissingRel.into();
        assert!(matches!(e, crate::CdbError::Link(_)));
    }
}
