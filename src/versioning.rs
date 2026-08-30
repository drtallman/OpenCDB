//! CDB Versioning Requirements Module (spec §7.14) — the pure model.
//!
//! Implements the `/req/core/versioning` requirements class (V1–V6,
//! §7.14.2–§7.14.7): the versioning-collection unit, its persisted
//! manifest, and the inverse algebra behind rollback. All I/O — applying
//! collections, archiving prior bytes, reading the journal, rolling back —
//! lives on the [`crate::datastore::CdbDatastore`] facade; this module is
//! types and rules only, the same split as every other requirements module.
//!
//! Draft quirks, doc-noted keyed on meaning:
//! - V1's box URI `req/core/versioning` collides with the requirements
//!   CLASS URI verbatim (cite §7.14.2 beside V1 — the Face1/Topo1 lesson).
//! - V2: the class table says `version-collection`, the box says
//!   `versioning-collection`, and the box text is a strict subset of V1's
//!   (it never mentions collections); §7.14.3's prose defines the concept
//!   ("a revision … may include one to many changes"; "the set of changes
//!   is referred to as a versioning collection") — V2 binds the collection
//!   unit realized by [`PendingCollection`]/[`CollectionManifest`].
//! - V3 (§7.14.4) carries a raw editorial TODO in the normative text
//!   ("Need words on how to link the resource to additional information
//!   about the chnage to the asset, such as who did it and a
//!   description."): the manifest's `description` and each change's
//!   `resource_record` link supply that linkage, implementation-defined.
//! - V6's box drops a word ("the ability capture") — typo.
//! - Every box URI drops the leading `/` (cosmetic; cited as printed).
//!
//! This module deliberately has NO warning type — the second optional
//! class without one (after topology) — because §7.14 contains no
//! SHOULD-level finding. Full byte-level rollback is deliberately MORE
//! than the boxes require (apply + track): §7.14's introduction says the
//! versioning metadata "enables … rollback to previous versions of the
//! entire datastore or a given asset", and this crate realizes that
//! ability; the `versions/<id>/` archive layout is implementation-defined.
//! Note: rolling back a change with a linked resource record restamps
//! that record's `updated` to the rollback instant (V3 applies to
//! rollbacks too) — the one field where a restore is deliberately not
//! byte-identical to the archived prior.
//! Also deliberate: one change per asset per collection (keeps archives
//! and inverses well-defined), free-form state strings (profiles may
//! restrict), and no archive pruning (operator business).

use std::collections::BTreeSet;
use std::fmt;
use std::io;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::metadata::temporal;
use crate::naming::NamingViolation;

/// Name of the datastore directory holding one immutable
/// `<collection id>/` entry per applied collection (manifest + archived
/// prior bytes). Reserved in [`crate::naming::StyleGuide`] the way
/// `global_metadata` is.
pub const VERSIONS_DIR: &str = "versions";

/// Highest representable collection sequence: the `v######` id space.
pub const SEQUENCE_MAX: u32 = 999_999;

/// A violation of a SHALL requirement of the versioning module (§7.14),
/// or of one of its doc-noted implementation constraints.
#[derive(Debug, Error, Clone, PartialEq)]
#[non_exhaustive]
pub enum VersioningViolation {
    /// §7.14.3: a revision includes "one to many changes" — an empty
    /// collection is not a versioning collection (meaning-keyed).
    #[error(
        "a versioning collection must contain at least one change (violates req/core/versioning-collection — table slug version-collection — §7.14.3)"
    )]
    EmptyCollection,
    /// Implementation constraint (doc-noted, §7.14.3): one change per
    /// asset per collection keeps archives and inverses well-defined.
    #[error(
        "asset {asset:?} appears in more than one change of the collection; one change per asset per collection (implementation constraint under req/core/versioning-collection, §7.14.3)"
    )]
    DuplicateAssetInCollection { asset: String },
    /// Requirement V6 (§7.14.7): a captured change of state needs a plain
    /// value — not empty, blank, or containing control characters.
    #[error(
        "state for asset {asset:?} is empty, blank, or contains control characters; a captured change of state needs a plain value (violates req/core/versioning-transitory, §7.14.7)"
    )]
    EmptyState { asset: String },
    /// Requirement V1 (§7.14.2): changes address assets by datastore
    /// logical path; validation delegates to the naming module.
    #[error(
        "asset path {asset:?} is not a valid datastore logical path (req/core/versioning, §7.14.2): {source}"
    )]
    InvalidAssetPath {
        asset: String,
        source: NamingViolation,
    },
    /// Implementation constraint (doc-noted): the `versions/` journal and
    /// the `global_metadata/` records are crate-managed machinery — not
    /// assets a collection may address. Global-metadata changes go
    /// through `write_global_metadata` (and V3-B touches it on every
    /// apply); the journal is written only by the apply pipeline.
    #[error(
        "path {asset:?} is inside the reserved {tree:?} tree, which versioning collections may not address (implementation constraint under req/core/versioning, §7.14.2)"
    )]
    AssetInReservedTree { asset: String, tree: String },
    /// The `v######` id space is 1-based and finite; sequence 0 and
    /// sequences beyond [`SEQUENCE_MAX`] are refused (§7.14.3).
    #[error(
        "collection sequence {sequence} is outside 1..={SEQUENCE_MAX}; the v micro-id space is exhausted (req/core/versioning-collection, §7.14.3)"
    )]
    SequenceExhausted { sequence: u32 },
    /// Requirement V4-A (§7.14.5): creating an asset that already exists.
    #[error(
        "cannot create {asset:?}: the asset already exists (req/core/versioning-functions A, §7.14.5)"
    )]
    AssetAlreadyExists { asset: String },
    /// Requirements V4-B/C, V5, V6 (§7.14.5–§7.14.7): the addressed asset
    /// does not exist in the datastore.
    #[error(
        "asset {asset:?} does not exist in the datastore (req/core/versioning-functions B/C, -file-replacement, -transitory, §7.14.5–§7.14.7)"
    )]
    AssetMissing { asset: String },
    /// Requirement V6 (§7.14.7): clearing a state requires one to be set.
    #[error("asset {asset:?} has no state to clear (req/core/versioning-transitory, §7.14.7)")]
    AssetStateMissing { asset: String },
    /// Requirement V3-C (§7.14.4) updates an EXISTING record's `Updated`
    /// element; a linked record that is absent cannot be updated.
    #[error(
        "resource metadata record {record:?} does not exist; Versioning3-C updates an existing record's Updated element (req/core/versioning-metadata, §7.14.4)"
    )]
    ResourceRecordMissing { record: String },
    /// Journal integrity: inverse chains are only sound over an unbroken,
    /// 1-based, contiguous journal (§7.14.3).
    #[error(
        "versioning journal is not contiguous: expected sequence {expected}, found {found}; inverse chains need an unbroken journal (req/core/versioning-collection, §7.14.3)"
    )]
    ManifestSequenceGap { expected: u32, found: u32 },
    /// A rollback target that is not in the journal (§7.14.2).
    #[error("no versioning collection {id} exists in the journal (req/core/versioning, §7.14.2)")]
    UnknownCollection { id: String },
    /// Implementation constraint (doc-noted): only the newest collection
    /// may be rolled back directly — undoing an older one beneath newer
    /// work would restore stale bytes. Whole-tail rollback goes through
    /// `rollback_to`.
    #[error(
        "collection {id} is not the latest ({latest}); only the newest collection can be rolled back directly — use rollback_to (implementation constraint, §7.14)"
    )]
    NotLatestCollection { id: String, latest: String },
}

/// Operational failure of the versioning module: an I/O or encoding
/// failure, or a [`VersioningViolation`]. The error/violation split
/// mirrors `hierarchy`'s. Metadata rewrites the apply pipeline performs
/// (V3-B/C) surface through their own module's error family instead.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum VersioningError {
    /// A SHALL violation (§7.14).
    #[error(transparent)]
    Violation(#[from] VersioningViolation),
    /// Filesystem failure while archiving, mutating, or journaling.
    #[error("versioning I/O failure: {0}")]
    Io(#[from] io::Error),
    /// Manifest (de)serialization failure.
    #[error("versioning manifest encoding failure: {0}")]
    Serialization(String),
}

/// Identifier of an applied versioning collection: `v` + a zero-padded
/// 6-digit, 1-based sequence (`v000001`). Lexicographic order equals
/// chronological order, and the rendering is a naming-valid path
/// component by construction (§7.14.3; the id structure is
/// implementation-defined — the spec's V2 prose leaves it open).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CollectionId {
    sequence: u32,
}

impl CollectionId {
    /// Builds the id for a 1-based sequence; 0 and values beyond
    /// [`SEQUENCE_MAX`] are [`VersioningViolation::SequenceExhausted`].
    pub fn from_sequence(sequence: u32) -> Result<CollectionId, VersioningViolation> {
        if sequence == 0 || sequence > SEQUENCE_MAX {
            return Err(VersioningViolation::SequenceExhausted { sequence });
        }
        Ok(CollectionId { sequence })
    }

    /// The 1-based sequence.
    pub fn sequence(self) -> u32 {
        self.sequence
    }

    /// Parses the strict `v######` rendering; anything else is `None`
    /// (the journal scan skips stray directory names, documented on
    /// `CdbDatastore::versions`).
    pub fn parse(value: &str) -> Option<CollectionId> {
        let digits = value.strip_prefix('v')?;
        if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let sequence: u32 = digits.parse().ok()?;
        CollectionId::from_sequence(sequence).ok()
    }
}

/// Renders the `v######` form (e.g. `v000001`).
impl fmt::Display for CollectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{:06}", self.sequence)
    }
}

/// Serializes as the `v######` string.
impl serde::Serialize for CollectionId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// Deserializes from the strict `v######` string.
impl<'de> serde::Deserialize<'de> for CollectionId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        CollectionId::parse(&value).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "collection id {value:?} is not the v<six digits> form"
            ))
        })
    }
}

/// The kind of applied change a [`ChangeRecord`] documents (V1/V4's CRUD
/// plus V6's state events). Unit-only with the state value carried beside
/// it on the record — a flat shape both JSON and XML encode cleanly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeAction {
    /// "created" — V4-A (§7.14.5).
    Created,
    /// "replaced" — V4-C / V5 (§7.14.5–§7.14.6).
    Replaced,
    /// "deleted" — V4-B (§7.14.5).
    Deleted,
    /// "state-set" — V6 (§7.14.7); the value is [`ChangeRecord::state`].
    StateSet,
    /// "state-cleared" — V6 (§7.14.7); clearing IS a change of state.
    StateCleared,
}

impl ChangeAction {
    /// Every action, in CRUD-then-state order.
    pub const ALL: [ChangeAction; 5] = [
        ChangeAction::Created,
        ChangeAction::Replaced,
        ChangeAction::Deleted,
        ChangeAction::StateSet,
        ChangeAction::StateCleared,
    ];

    /// The wire spelling used in manifests.
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeAction::Created => "created",
            ChangeAction::Replaced => "replaced",
            ChangeAction::Deleted => "deleted",
            ChangeAction::StateSet => "state-set",
            ChangeAction::StateCleared => "state-cleared",
        }
    }

    /// Parses a wire spelling; unknown values fail manifest decoding.
    pub fn parse(value: &str) -> Option<ChangeAction> {
        ChangeAction::ALL
            .into_iter()
            .find(|action| action.as_str() == value)
    }
}

/// Serializes as the wire string (e.g. "state-set").
impl serde::Serialize for ChangeAction {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// Deserializes from the wire string; unknown values error.
impl<'de> serde::Deserialize<'de> for ChangeAction {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        ChangeAction::parse(&value).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "change action {value:?} is not created|replaced|deleted|state-set|state-cleared"
            ))
        })
    }
}

/// Displays as the wire string.
impl fmt::Display for ChangeAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One documented change of an applied collection (V2/V3, §7.14.3–.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeRecord {
    /// Datastore logical path of the changed asset.
    pub asset: String,
    /// What happened to it.
    pub action: ChangeAction,
    /// The state value set — present exactly when `action` is
    /// [`ChangeAction::StateSet`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// Logical path of the asset's resource metadata record whose
    /// `updated` element the apply refreshed (V3-C); the linkage the
    /// spec's own §7.14.4 editorial TODO admits it lacks words for.
    #[serde(
        rename = "resourceRecord",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub resource_record: Option<String>,
    /// Whether this collection's directory archives the asset's prior
    /// bytes (true for `Replaced`/`Deleted`).
    pub archived: bool,
    /// The asset's state before this change — captured at apply time so
    /// rollback inverts a state event from the manifest alone. Present
    /// only on state actions that had a prior state.
    #[serde(
        rename = "priorState",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub prior_state: Option<String>,
}

/// The persisted journal entry of one applied collection:
/// `versions/<id>/manifest.<enc>` in the datastore's declared encoding
/// (V2/V3, §7.14.3–.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollectionManifest {
    /// The collection's identifier (`v######`).
    pub id: CollectionId,
    /// The 1-based sequence (redundant with `id`, kept human-checkable).
    pub sequence: u32,
    /// When the collection was applied — the single instant that also
    /// lands in the global `update` and every touched record's `updated`
    /// (V3-A, §7.14.4).
    #[serde(with = "temporal::rfc3339_utc")]
    pub applied: DateTime<Utc>,
    /// Free-text description of the revision (the spec's §7.14.4 TODO
    /// gap, filled implementation-defined).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The documented changes, in application order.
    pub changes: Vec<ChangeRecord>,
}

/// One inverse operation computed from a manifest (pure — the facade
/// fills `Restore*` bytes from the collection's archive).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InverseOp {
    /// Undo of `Created`: delete the asset.
    Delete {
        asset: String,
        resource_record: Option<String>,
    },
    /// Undo of `Replaced`: replace with this collection's archived bytes.
    RestoreReplace {
        asset: String,
        resource_record: Option<String>,
    },
    /// Undo of `Deleted`: re-create from this collection's archived bytes.
    RestoreCreate {
        asset: String,
        resource_record: Option<String>,
    },
    /// Undo of a state action whose prior state existed.
    SetState {
        asset: String,
        state: String,
        resource_record: Option<String>,
    },
    /// Undo of a first-ever `StateSet` (no prior state to restore).
    ClearState {
        asset: String,
        resource_record: Option<String>,
    },
}

impl CollectionManifest {
    /// Serializes the manifest as pretty JSON.
    pub fn to_json_string(&self) -> Result<String, VersioningError> {
        serde_json::to_string_pretty(self)
            .map_err(|e| VersioningError::Serialization(e.to_string()))
    }

    /// Parses a JSON manifest.
    pub fn from_json_str(content: &str) -> Result<CollectionManifest, VersioningError> {
        serde_json::from_str(content).map_err(|e| VersioningError::Serialization(e.to_string()))
    }

    /// Serializes the manifest as XML.
    pub fn to_xml_string(&self) -> Result<String, VersioningError> {
        quick_xml::se::to_string(self).map_err(|e| VersioningError::Serialization(e.to_string()))
    }

    /// Parses an XML manifest.
    pub fn from_xml_str(content: &str) -> Result<CollectionManifest, VersioningError> {
        quick_xml::de::from_str(content).map_err(|e| VersioningError::Serialization(e.to_string()))
    }

    /// The pure inverse of this collection, in change order — the closed
    /// algebra behind rollback: `Created` → delete; `Replaced` → restore
    /// archived bytes as a replace; `Deleted` → restore archived bytes as
    /// a create; `StateSet` with a prior → set the prior; `StateSet`
    /// without one → clear; `StateCleared` → set the recorded prior
    /// (always present for well-formed manifests; the total fallback
    /// clears). Each inverse carries the original change's
    /// `resource_record` link so V3 timestamps refresh on rollbacks too.
    pub fn inverse_ops(&self) -> Vec<InverseOp> {
        self.changes
            .iter()
            .map(|change| {
                let asset = change.asset.clone();
                let resource_record = change.resource_record.clone();
                match change.action {
                    ChangeAction::Created => InverseOp::Delete {
                        asset,
                        resource_record,
                    },
                    ChangeAction::Replaced => InverseOp::RestoreReplace {
                        asset,
                        resource_record,
                    },
                    ChangeAction::Deleted => InverseOp::RestoreCreate {
                        asset,
                        resource_record,
                    },
                    ChangeAction::StateSet | ChangeAction::StateCleared => {
                        match change.prior_state.clone() {
                            Some(state) => InverseOp::SetState {
                                asset,
                                state,
                                resource_record,
                            },
                            None => InverseOp::ClearState {
                                asset,
                                resource_record,
                            },
                        }
                    }
                }
            })
            .collect()
    }
}

/// The state of `asset` after replaying the given manifests in order:
/// the last state action wins (`StateSet` → its value, `StateCleared` →
/// none); `None` if the asset was never state-set. Pure — V6's
/// "view changes over time" over any journal slice (§7.14.7).
pub fn state_from_manifests<'a, I>(manifests: I, asset: &str) -> Option<&'a str>
where
    I: IntoIterator<Item = &'a CollectionManifest>,
{
    let mut current = None;
    for manifest in manifests {
        for change in &manifest.changes {
            if change.asset == asset {
                match change.action {
                    ChangeAction::StateSet => current = change.state.as_deref(),
                    ChangeAction::StateCleared => current = None,
                    _ => {}
                }
            }
        }
    }
    current
}

/// If `path`'s first component names a reserved, crate-managed tree —
/// the `versions/` journal ([`VERSIONS_DIR`]) or the `global_metadata/`
/// records ([`crate::hierarchy::GLOBAL_METADATA_DIR`]) — returns that
/// component; otherwise `None`. Versioning collections may not address
/// either tree (implementation constraint, §7.14.2): see
/// [`VersioningViolation::AssetInReservedTree`].
pub(crate) fn reserved_tree_of(path: &str) -> Option<&'static str> {
    let first = path.trim_start_matches('/').split('/').next()?;
    if first == VERSIONS_DIR {
        Some(VERSIONS_DIR)
    } else if first == crate::hierarchy::GLOBAL_METADATA_DIR {
        Some(crate::hierarchy::GLOBAL_METADATA_DIR)
    } else {
        None
    }
}

/// One not-yet-applied change of a [`PendingCollection`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingChange {
    pub(crate) asset: String,
    pub(crate) op: PendingOp,
    pub(crate) resource_record: Option<String>,
}

/// The operation a [`PendingChange`] performs; `Create`/`Replace` carry
/// the bytes to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PendingOp {
    Create { bytes: Vec<u8> },
    Replace { bytes: Vec<u8> },
    Delete,
    SetState { state: String },
    ClearState,
}

/// A versioning collection under construction (Requirement V2, §7.14.3):
/// the "one to many changes" unit the facade applies atomically-in-order
/// via `CdbDatastore::apply_collection`. Builder-style; each `for_record`
/// call attaches a resource-record link (V3-C) to the MOST RECENTLY added
/// change (calling it before any change is a documented no-op).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PendingCollection {
    pub(crate) description: Option<String>,
    pub(crate) changes: Vec<PendingChange>,
}

impl PendingCollection {
    /// An empty collection (invalid until a change is added).
    pub fn new() -> PendingCollection {
        PendingCollection::default()
    }

    /// Free-text description of the revision (§7.14.4's admitted gap).
    pub fn description(mut self, value: impl Into<String>) -> Self {
        self.description = Some(value.into());
        self
    }

    /// Adds a create-asset change (V4-A, §7.14.5).
    pub fn create(mut self, asset: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        self.changes.push(PendingChange {
            asset: asset.into(),
            op: PendingOp::Create {
                bytes: bytes.into(),
            },
            resource_record: None,
        });
        self
    }

    /// Adds a replace-asset change (V4-C/V5, §7.14.5–§7.14.6).
    pub fn replace(mut self, asset: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        self.changes.push(PendingChange {
            asset: asset.into(),
            op: PendingOp::Replace {
                bytes: bytes.into(),
            },
            resource_record: None,
        });
        self
    }

    /// Adds a delete-asset change (V4-B, §7.14.5).
    pub fn delete(mut self, asset: impl Into<String>) -> Self {
        self.changes.push(PendingChange {
            asset: asset.into(),
            op: PendingOp::Delete,
            resource_record: None,
        });
        self
    }

    /// Adds a set-state change (V6, §7.14.7).
    pub fn set_state(mut self, asset: impl Into<String>, state: impl Into<String>) -> Self {
        self.changes.push(PendingChange {
            asset: asset.into(),
            op: PendingOp::SetState {
                state: state.into(),
            },
            resource_record: None,
        });
        self
    }

    /// Adds a clear-state change (V6, §7.14.7 — clearing IS a change of
    /// state); valid only for an asset whose state is currently set.
    pub fn clear_state(mut self, asset: impl Into<String>) -> Self {
        self.changes.push(PendingChange {
            asset: asset.into(),
            op: PendingOp::ClearState,
            resource_record: None,
        });
        self
    }

    /// Links the MOST RECENTLY added change to the resource metadata
    /// record whose `updated` element the apply refreshes (V3-C,
    /// §7.14.4). Calling this before any change was added is a
    /// documented no-op.
    pub fn for_record(mut self, resource_record: impl Into<String>) -> Self {
        if let Some(last) = self.changes.last_mut() {
            last.resource_record = Some(resource_record.into());
        }
        self
    }

    /// Validates the collection's own invariants (everything checkable
    /// without a datastore): non-empty (§7.14.3), valid logical paths
    /// (naming delegation), one change per asset, non-empty states.
    pub fn validate(&self) -> Result<(), VersioningViolation> {
        if self.changes.is_empty() {
            return Err(VersioningViolation::EmptyCollection);
        }
        let mut seen = BTreeSet::new();
        for change in &self.changes {
            for path in std::iter::once(&change.asset).chain(change.resource_record.iter()) {
                if let Err(source) = crate::hierarchy::logical_path(path) {
                    return Err(VersioningViolation::InvalidAssetPath {
                        asset: path.clone(),
                        source,
                    });
                }
            }
            if !seen.insert(change.asset.clone()) {
                return Err(VersioningViolation::DuplicateAssetInCollection {
                    asset: change.asset.clone(),
                });
            }
            if let PendingOp::SetState { state } = &change.op
                && (state.trim().is_empty() || state.chars().any(char::is_control))
            {
                return Err(VersioningViolation::EmptyState {
                    asset: change.asset.clone(),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn record(asset: &str, action: ChangeAction) -> ChangeRecord {
        ChangeRecord {
            asset: asset.to_owned(),
            action,
            state: None,
            resource_record: None,
            archived: matches!(action, ChangeAction::Replaced | ChangeAction::Deleted),
            prior_state: None,
        }
    }

    fn manifest(sequence: u32, applied: &str, changes: Vec<ChangeRecord>) -> CollectionManifest {
        CollectionManifest {
            id: CollectionId::from_sequence(sequence).unwrap(),
            sequence,
            applied: ts(applied),
            description: None,
            changes,
        }
    }

    /// §7.14.3 — collection ids are `v` + a 1-based zero-padded 6-digit
    /// sequence; the id space is finite and 0 is not a sequence.
    #[test]
    fn req_core_versioning_collection_id_format_and_cap() {
        let id = CollectionId::from_sequence(1).unwrap();
        assert_eq!(id.to_string(), "v000001");
        assert_eq!(id.sequence(), 1);
        assert_eq!(CollectionId::parse("v000001"), Some(id));
        assert_eq!(
            CollectionId::from_sequence(SEQUENCE_MAX)
                .unwrap()
                .to_string(),
            "v999999"
        );
        assert!(matches!(
            CollectionId::from_sequence(0),
            Err(VersioningViolation::SequenceExhausted { sequence: 0 })
        ));
        assert!(matches!(
            CollectionId::from_sequence(SEQUENCE_MAX + 1),
            Err(VersioningViolation::SequenceExhausted { .. })
        ));
        for bad in ["", "v1", "v0000001", "x000001", "v00000a", "v000000"] {
            assert_eq!(CollectionId::parse(bad), None, "must reject {bad:?}");
        }
    }

    /// Requirement V2 req/core/versioning-collection (§7.14.3, "one to
    /// many changes") — an empty collection is rejected.
    #[test]
    fn req_core_versioning_collection_requires_changes() {
        assert_eq!(
            PendingCollection::new().validate(),
            Err(VersioningViolation::EmptyCollection)
        );
        assert_eq!(
            PendingCollection::new().description("empty").validate(),
            Err(VersioningViolation::EmptyCollection)
        );
    }

    /// Doc-noted implementation constraint (§7.14.3) — at most one change
    /// per asset per collection.
    #[test]
    fn req_core_versioning_collection_rejects_duplicate_asset() {
        let pending = PendingCollection::new()
            .create("/Tiles/RoadNetwork.gpkg", *b"aa")
            .delete("/Tiles/RoadNetwork.gpkg");
        assert_eq!(
            pending.validate(),
            Err(VersioningViolation::DuplicateAssetInCollection {
                asset: "/Tiles/RoadNetwork.gpkg".to_owned(),
            })
        );
    }

    /// Requirement V6 req/core/versioning-transitory (§7.14.7) — a state
    /// value must be a plain non-empty string: empty, blank, and
    /// control-character states are all rejected.
    #[test]
    fn req_core_versioning_collection_rejects_empty_state() {
        for bad in ["", "  ", "a\u{1}b"] {
            let pending = PendingCollection::new().set_state("/Tiles/RoadNetwork.gpkg", bad);
            assert_eq!(
                pending.validate(),
                Err(VersioningViolation::EmptyState {
                    asset: "/Tiles/RoadNetwork.gpkg".to_owned(),
                }),
                "must reject state {bad:?}"
            );
        }
    }

    /// Requirement V1 req/core/versioning (§7.14.2) — asset and record
    /// paths must be valid datastore logical paths (naming delegation).
    #[test]
    fn req_core_versioning_collection_rejects_invalid_asset_path() {
        let pending = PendingCollection::new().delete("");
        assert!(matches!(
            pending.validate(),
            Err(VersioningViolation::InvalidAssetPath { asset, .. }) if asset.is_empty()
        ));
        let pending = PendingCollection::new()
            .create("/Tiles/RoadNetwork.gpkg", *b"aa")
            .for_record("");
        assert!(matches!(
            pending.validate(),
            Err(VersioningViolation::InvalidAssetPath { asset, .. }) if asset.is_empty()
        ));
    }

    /// V3-C linkage (§7.14.4) — `for_record` attaches to the most recently
    /// added change; before any change it is a documented no-op.
    #[test]
    fn req_core_versioning_builder_attaches_record_to_last_change() {
        let pending = PendingCollection::new()
            .create("/Tiles/RoadNetwork.gpkg", *b"aa")
            .for_record("/Tiles/metadata/RoadNetwork.json")
            .delete("/Tiles/Buildings.gpkg");
        assert_eq!(
            pending.changes[0].resource_record.as_deref(),
            Some("/Tiles/metadata/RoadNetwork.json")
        );
        assert_eq!(pending.changes[1].resource_record, None);
        let ignored = PendingCollection::new().for_record("/Tiles/metadata/RoadNetwork.json");
        assert!(ignored.changes.is_empty());
    }

    /// Requirements V2/V3 (§7.14.3–.4) — the manifest round-trips JSON
    /// with its wire names.
    #[test]
    fn req_core_versioning_manifest_json_roundtrip() {
        let mut set = record("/Tiles/RoadNetwork.gpkg", ChangeAction::StateSet);
        set.state = Some("closed".to_owned());
        set.prior_state = Some("open".to_owned());
        set.resource_record = Some("/Tiles/metadata/RoadNetwork.json".to_owned());
        let subject = manifest(
            1,
            "2026-08-30T12:00:00Z",
            vec![record("/Tiles/Buildings.gpkg", ChangeAction::Replaced), set],
        );
        let json = subject.to_json_string().unwrap();
        assert!(json.contains("\"v000001\""), "id wire form: {json}");
        assert!(json.contains("\"resourceRecord\""), "wire name: {json}");
        assert!(json.contains("\"priorState\""), "wire name: {json}");
        assert!(json.contains("state-set"), "action wire form: {json}");
        assert!(
            json.contains("2026-08-30T12:00:00Z"),
            "RFC 3339 UTC: {json}"
        );
        let back = CollectionManifest::from_json_str(&json).unwrap();
        assert_eq!(back, subject);
    }

    /// Requirements V2/V3 (§7.14.3–.4) — the manifest round-trips XML
    /// (the declared-encoding journal must work for XML datastores too).
    #[test]
    fn req_core_versioning_manifest_xml_roundtrip() {
        let mut cleared = record("/Tiles/RoadNetwork.gpkg", ChangeAction::StateCleared);
        cleared.prior_state = Some("closed".to_owned());
        let subject = manifest(
            2,
            "2026-08-30T13:30:00Z",
            vec![
                record("/Tiles/Buildings.gpkg", ChangeAction::Deleted),
                cleared,
            ],
        );
        let xml = subject.to_xml_string().unwrap();
        let back = CollectionManifest::from_xml_str(&xml).unwrap();
        assert_eq!(back, subject);
    }

    /// §7.14 — the action wire spellings are exact and closed.
    #[test]
    fn req_core_versioning_change_action_wire_spellings() {
        let expected = [
            (ChangeAction::Created, "created"),
            (ChangeAction::Replaced, "replaced"),
            (ChangeAction::Deleted, "deleted"),
            (ChangeAction::StateSet, "state-set"),
            (ChangeAction::StateCleared, "state-cleared"),
        ];
        assert_eq!(ChangeAction::ALL.len(), expected.len());
        for (action, wire) in expected {
            assert_eq!(action.as_str(), wire);
            assert_eq!(action.to_string(), wire);
            assert_eq!(ChangeAction::parse(wire), Some(action));
        }
        assert_eq!(ChangeAction::parse("bogus"), None);
    }

    /// Rollback algebra (beyond-boxes, §7.14 intro) — CRUD inverses:
    /// Created→Delete, Replaced→RestoreReplace, Deleted→RestoreCreate,
    /// with the resource-record link carried through.
    #[test]
    fn req_core_versioning_inverse_ops_crud() {
        let mut created = record("/Tiles/A.gpkg", ChangeAction::Created);
        created.resource_record = Some("/Tiles/metadata/A.json".to_owned());
        let subject = manifest(
            3,
            "2026-08-30T14:00:00Z",
            vec![
                created,
                record("/Tiles/B.gpkg", ChangeAction::Replaced),
                record("/Tiles/C.gpkg", ChangeAction::Deleted),
            ],
        );
        assert_eq!(
            subject.inverse_ops(),
            vec![
                InverseOp::Delete {
                    asset: "/Tiles/A.gpkg".to_owned(),
                    resource_record: Some("/Tiles/metadata/A.json".to_owned()),
                },
                InverseOp::RestoreReplace {
                    asset: "/Tiles/B.gpkg".to_owned(),
                    resource_record: None,
                },
                InverseOp::RestoreCreate {
                    asset: "/Tiles/C.gpkg".to_owned(),
                    resource_record: None,
                },
            ]
        );
    }

    /// Rollback algebra for states (V6, §7.14.7) — a prior state inverts
    /// to `SetState(prior)`; a first-ever set inverts to `ClearState`; a
    /// clear inverts to `SetState(prior)`.
    #[test]
    fn req_core_versioning_inverse_ops_states() {
        let mut with_prior = record("/Tiles/A.gpkg", ChangeAction::StateSet);
        with_prior.state = Some("closed".to_owned());
        with_prior.prior_state = Some("open".to_owned());
        let mut first_ever = record("/Tiles/B.gpkg", ChangeAction::StateSet);
        first_ever.state = Some("closed".to_owned());
        let mut cleared = record("/Tiles/C.gpkg", ChangeAction::StateCleared);
        cleared.prior_state = Some("flooded".to_owned());
        let subject = manifest(
            4,
            "2026-08-30T15:00:00Z",
            vec![with_prior, first_ever, cleared],
        );
        assert_eq!(
            subject.inverse_ops(),
            vec![
                InverseOp::SetState {
                    asset: "/Tiles/A.gpkg".to_owned(),
                    state: "open".to_owned(),
                    resource_record: None,
                },
                InverseOp::ClearState {
                    asset: "/Tiles/B.gpkg".to_owned(),
                    resource_record: None,
                },
                InverseOp::SetState {
                    asset: "/Tiles/C.gpkg".to_owned(),
                    state: "flooded".to_owned(),
                    resource_record: None,
                },
            ]
        );
    }

    /// Requirement V6 (§7.14.7) — the state view replays state actions in
    /// order; the last one wins and clears yield `None`.
    #[test]
    fn req_core_versioning_state_from_manifests_timeline() {
        let mut set_closed = record("/Tiles/A.gpkg", ChangeAction::StateSet);
        set_closed.state = Some("closed".to_owned());
        let mut cleared = record("/Tiles/A.gpkg", ChangeAction::StateCleared);
        cleared.prior_state = Some("closed".to_owned());
        let mut set_flooded = record("/Tiles/A.gpkg", ChangeAction::StateSet);
        set_flooded.state = Some("flooded".to_owned());
        let journal = [
            manifest(1, "2026-08-30T10:00:00Z", vec![set_closed]),
            manifest(2, "2026-08-30T11:00:00Z", vec![cleared]),
            manifest(3, "2026-08-30T12:00:00Z", vec![set_flooded]),
        ];
        assert_eq!(
            state_from_manifests(&journal[..1], "/Tiles/A.gpkg"),
            Some("closed")
        );
        assert_eq!(state_from_manifests(&journal[..2], "/Tiles/A.gpkg"), None);
        assert_eq!(
            state_from_manifests(&journal, "/Tiles/A.gpkg"),
            Some("flooded")
        );
        assert_eq!(state_from_manifests(&journal, "/Tiles/B.gpkg"), None);
    }

    /// §7.14 — the versioning families route through the crate-wide error
    /// taxonomy like every other requirements module.
    #[test]
    fn versioning_violation_converts_into_cdb_error() {
        let err = crate::error::CdbError::from(VersioningError::from(
            VersioningViolation::EmptyCollection,
        ));
        assert!(matches!(
            err,
            crate::error::CdbError::Versioning(VersioningError::Violation(
                VersioningViolation::EmptyCollection
            ))
        ));
    }
}
