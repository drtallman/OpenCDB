//! CDB Topology Core Requirements Module (spec §7.13).
//!
//! Implements the `/req/core/topology` requirements class (Topo1–6 +
//! Rec Topology 1, §7.13.4) and the optional Face class
//! `/req/core/topology-face` (Face1–4, §7.13.5). Topological primitives
//! are ISO 19107:2017 constructs — equivalent to but NOT subclasses of
//! geometric primitives (§7.13.1 NOTE) — so [`TopoNode`], [`TopoEdge`],
//! and [`TopoFace`] are their own types, separate from the geometry
//! module, carrying at most a 2D direct position / linestring
//! realization (x = longitude, y = latitude, the geometry module's
//! geo-types convention); Z/M realizations are the geometry module's
//! business.
//!
//! Draft quirks, doc-noted keyed on meaning (the §7.11/§7.12 lesson —
//! always cite the section beside a requirement id):
//! - Topo1's and Face1's boxes cite "ISO 19101:2017" where the heading,
//!   class table, and section title all say ISO 19107:2017 (Spatial
//!   Schema); 19107 governs. Face1 (§7.13.5.2) reuses Topo1's URI
//!   `/req/core/topology-topic1` verbatim.
//! - The class table spells `-nodeid`/`-edgeid`/`-faceid`; the boxes say
//!   `-nodeID`/`-edgeID`/`-faceID`. §7.13.4.3's prose says "each node"
//!   where it means "each edge".
//! - Topo6's box URI is `/req/core/topology-clip`; the class table says
//!   `-edge-clip`.
//! - Face3/Face4 are labeled "Face Topology Requirement" with SHALL text
//!   but carry `/rec/` URIs (and Face4's slug differs between box,
//!   `-face-winding`, and table, `-winding`): treated as conditional
//!   SHALLs — hard errors once faces are generated.
//! - Rec Topology 1 (`/rec/core/topology-face`, §7.13.4.6) duplicates
//!   Face3 verbatim; both are satisfied structurally by [`TopoFace`] (a
//!   face IS a list of directed edges with a derived directed-node list)
//!   and can never fire at runtime.
//!
//! This module deliberately has NO warning type — the first optional
//! class without one — because §7.13 contains no SHOULD-level dataset
//! finding: the islands/holes "should" (§7.13.5.6) is unboxed profile
//! guidance, recorded on [`TopoFace`]'s rustdoc instead. Also
//! deliberate: no serde on the primitives or [`TopoGraph`] (the spec
//! defines no topology encoding; revisit at 14b), no face/polygon
//! clipping ([`TopologyViolation::EdgeInFace`] guards §7.13.4.7's
//! edge-only scope), no traversal/auto-noding, and no public delete API.
//!
//! The primitives are a **serde surface**, wanted since Phase 11 so a vector
//! tile can carry its topology: [`TopoNode`], [`TopoEdge`], [`TopoFace`],
//! [`WindingOrder`] and the whole [`TopoGraph`] round-trip. Identifiers are
//! transparent integers on the wire and [`WindingOrder`] is the spec's own
//! spelling, so a serialized graph and a metadata record agree letter for
//! letter. Derived views — the Topo4 adjacency, [`DirectedNode`], the clip
//! mint counters — are deliberately absent from the wire and regenerated on
//! read, which is why deserializing a graph re-runs the `insert_*`
//! invariants instead of trusting the document, and why a document carrying
//! one of them is rejected rather than silently stripped.
//!
//! **Two wire fields this crate does not own.** [`TopoNode::position`] and
//! [`TopoEdge::geometry`] are `geo-types` values, and they serialize in
//! `geo-types`' own native coordinate form — `{"x":…,"y":…}` per coordinate,
//! a point as one such object and a linestring as an array of them. It is
//! not GeoJSON and not WKT. Because that shape is part of this crate's
//! public wire surface and freezes with it, `geo-types` is pinned as an API
//! dependency rather than an implementation detail: a release changing
//! `Coord`'s serde representation is a breaking change *for this crate*,
//! and `req_core_topology_primitives_serde_round_trip` asserts the exact
//! shape so such a release cannot pass the suite unnoticed.

use std::collections::BTreeMap;
use std::fmt;

use geo::Intersects;
use geo_types::{Coord, Rect, coord};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::metadata::{Bbox, MetadataViolation, ResourceMetadata};

/// Unique node identifier (Requirement Topo2,
/// /req/core/topology-nodeID, §7.13.4.2; the class table spells the slug
/// `-nodeid`). The spec leaves ID structure open ("an integer number or
/// a combination of a tile identifier and an integer number"); the core
/// picks plain integers and leaves tile-scoped composition to
/// application profiles. Crosswalk: CDB 1.x SJID/EJID/JID (§7.13.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u64);

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique edge identifier (Requirement Topo3,
/// /req/core/topology-edgeID, §7.13.4.3; table slug `-edgeid`; the
/// section prose's "each node" means "each edge" — draft quirk).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EdgeId(pub u64);

impl fmt::Display for EdgeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique face identifier (Face Topology Requirement 2,
/// /req/core/topology-faceID, §7.13.5.3; table slug `-faceid`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FaceId(pub u64);

impl fmt::Display for FaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A directed-node entry: an edge incident to a node, marked with the
/// node's role for it (Requirement Topo4, /req/core/topology-node-dir,
/// §7.13.4.4: "-" if the edge is leaving the node, "+" if the edge
/// enters it — consistent with ISO 19107's "result = end − start").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignedEdge {
    /// The edge leaves this node (the node is its start) — rendered "−".
    Leaving(EdgeId),
    /// The edge enters this node (the node is its end) — rendered "+".
    Entering(EdgeId),
}

impl SignedEdge {
    /// The underlying edge, sign stripped.
    pub fn edge(self) -> EdgeId {
        match self {
            SignedEdge::Leaving(edge) | SignedEdge::Entering(edge) => edge,
        }
    }
}

/// Renders the spec's exact marking, e.g. `-17` leaving, `+17` entering.
impl fmt::Display for SignedEdge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SignedEdge::Leaving(edge) => write!(f, "-{edge}"),
            SignedEdge::Entering(edge) => write!(f, "+{edge}"),
        }
    }
}

/// Orientation of an edge *use* relative to the edge's own start→end
/// direction (ISO 19107 directed edge, §7.13.1: "+" agrees with the
/// edge's orientation, "−" opposes it). A deliberately different type
/// from [`SignedEdge`] — conflating the node-centric signs with the
/// edge-use orientations was CDB 1.x's mistake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    /// Traverse the edge start→end.
    Forward,
    /// Traverse the edge end→start.
    Reverse,
}

/// ISO 19107 directed edge: an association between an edge and one of
/// its two orientations (§7.13.1). Face boundaries are lists of these
/// (Face Topology Requirement 3, §7.13.5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectedEdge {
    pub edge: EdgeId,
    pub orientation: Orientation,
}

impl DirectedEdge {
    /// Traversal endpoints (from, to) of this directed edge over `edge`:
    /// `Forward` goes start→end, `Reverse` end→start. Branches on the
    /// orientation, never on node equality — a loop edge's endpoints are
    /// equal and would make equality-based derivation ambiguous.
    fn endpoints(self, edge: &TopoEdge) -> (NodeId, NodeId) {
        match self.orientation {
            Orientation::Forward => (edge.start, edge.end),
            Orientation::Reverse => (edge.end, edge.start),
        }
    }
}

/// The role a node plays for an underlying edge (ISO 19107 directed
/// node, §7.13.1): `Start` renders "−", `End` renders "+".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeSign {
    /// The node is the underlying edge's start — "−".
    Start,
    /// The node is the underlying edge's end — "+".
    End,
}

/// ISO 19107 directed node: a node and its sign with respect to an
/// underlying edge (§7.13.1). Derived by [`TopoFace::directed_nodes`];
/// never stored, so it can never disagree with the edge list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectedNode {
    pub node: NodeId,
    pub sign: NodeSign,
}

/// Renders the spec's marking, e.g. `-5` start, `+5` end.
impl fmt::Display for DirectedNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.sign {
            NodeSign::Start => write!(f, "-{}", self.node),
            NodeSign::End => write!(f, "+{}", self.node),
        }
    }
}

/// 0-dimensional topological primitive (§7.13.1). `position` is the ISO
/// "direct position" — 2D, x = longitude, y = latitude — and `None` is
/// legal (a pure topology graph carries no coordinates, §7.13.2). A node
/// no edge references is an *isolated node*; with a position it is CDB
/// 1.x's point geometry (§7.13.3 crosswalk).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TopoNode {
    pub id: NodeId,
    pub position: Option<geo_types::Point<f64>>,
}

/// 1-dimensional topological primitive (§7.13.1). Start and end node
/// IDs are mandatory fields, so Requirement Topo5
/// (/req/core/topology-edge-dir, §7.13.4.5) holds by construction.
/// `geometry: None` is the spec's straight-line case ("It is possible
/// for two nodes to be connected with no geometry"); loops
/// (`start == end`) are permitted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TopoEdge {
    pub id: EdgeId,
    pub start: NodeId,
    pub end: NodeId,
    pub geometry: Option<geo_types::LineString<f64>>,
}

/// 2-dimensional topological primitive (§7.13.5): an exterior boundary
/// ring of directed edges — Face Topology Requirement 3's (and Rec
/// Topology 1's) "list of directed nodes and directed edges", with the
/// directed-node list derived by [`TopoFace::directed_nodes`] so the two
/// lists can never disagree. Islands/holes (interior boundaries,
/// §7.13.5.6) carry no requirement box and are an application-profile
/// duty — this core type models the exterior ring only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopoFace {
    pub id: FaceId,
    pub boundary: Vec<DirectedEdge>,
}

impl TopoFace {
    /// The ring as ISO directed nodes (Face Topology Requirement 3 and
    /// Rec Topology 1's "list of directed nodes and directed edges",
    /// §7.13.5.4/§7.13.4.6): per boundary step the traversal's (from,
    /// to) pair signed relative to the step's *underlying edge* —
    /// `Forward` yields (−start, +end), `Reverse` yields (+end, −start)
    /// — 2k entries for a k-edge ring. Derived, never stored, so the two
    /// lists can never disagree. Fallible because a `TopoFace` value can
    /// be built before insertion: unknown edges error.
    pub fn directed_nodes(
        &self,
        graph: &TopoGraph,
    ) -> Result<Vec<DirectedNode>, TopologyViolation> {
        let mut out = Vec::with_capacity(self.boundary.len() * 2);
        for step in &self.boundary {
            let edge = graph
                .edge(step.edge)
                .ok_or(TopologyViolation::UnknownEdgeId { id: step.edge })?;
            let (from, to) = match step.orientation {
                Orientation::Forward => (
                    DirectedNode {
                        node: edge.start,
                        sign: NodeSign::Start,
                    },
                    DirectedNode {
                        node: edge.end,
                        sign: NodeSign::End,
                    },
                ),
                Orientation::Reverse => (
                    DirectedNode {
                        node: edge.end,
                        sign: NodeSign::End,
                    },
                    DirectedNode {
                        node: edge.start,
                        sign: NodeSign::Start,
                    },
                ),
            };
            out.push(from);
            out.push(to);
        }
        Ok(out)
    }
}

/// Winding order of generated faces (Face Topology Requirement 4,
/// /req/core/topology-winding — box slug `-face-winding` — §7.13.5.5:
/// "either clockwise or counterclockwise"; the core does not pick one).
/// Declared on the dataset's resource metadata as the `windingOrder`
/// conditional element; see [`validate_topology_dataset`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindingOrder {
    /// "clockwise"
    Clockwise,
    /// "counterclockwise"
    Counterclockwise,
}

impl WindingOrder {
    /// Both winding orders, in §7.13.5.5's listing order.
    pub const ALL: [WindingOrder; 2] = [WindingOrder::Clockwise, WindingOrder::Counterclockwise];

    /// The wire spelling used on the `windingOrder` metadata element.
    pub fn as_str(self) -> &'static str {
        match self {
            WindingOrder::Clockwise => "clockwise",
            WindingOrder::Counterclockwise => "counterclockwise",
        }
    }

    /// Parses a wire spelling; unknown values violate Face4.
    pub fn parse(value: &str) -> Result<WindingOrder, TopologyViolation> {
        WindingOrder::ALL
            .into_iter()
            .find(|winding| winding.as_str() == value)
            .ok_or_else(|| TopologyViolation::UnknownWindingOrder {
                value: value.to_owned(),
            })
    }
}

/// Serializes as the spec wire string (e.g. "clockwise").
impl serde::Serialize for WindingOrder {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// Deserializes from the spec wire string; unknown values error.
impl<'de> serde::Deserialize<'de> for WindingOrder {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        WindingOrder::parse(&value).map_err(serde::de::Error::custom)
    }
}

/// Displays as the spec wire string.
impl fmt::Display for WindingOrder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A violation of a SHALL requirement of the topology module (§7.13).
/// Face3/Face4 are conditional SHALLs ("if faces are generated…") whose
/// boxes carry `/rec/` URI prefixes — draft quirks, treated per their
/// "Face Topology Requirement" labels and SHALL text.
#[derive(Debug, Error, Clone, PartialEq)]
#[non_exhaustive]
pub enum TopologyViolation {
    /// Requirement Topo2 (§7.13.4.2): node identifiers are unique.
    #[error(
        "node id {id} already exists in the graph (violates /req/core/topology-nodeID, §7.13.4.2)"
    )]
    DuplicateNodeId { id: NodeId },
    /// Requirement Topo3 (§7.13.4.3): edge identifiers are unique.
    #[error(
        "edge id {id} already exists in the graph (violates /req/core/topology-edgeID, §7.13.4.3)"
    )]
    DuplicateEdgeId { id: EdgeId },
    /// Face Topology Requirement 2 (§7.13.5.3): face identifiers are
    /// unique.
    #[error(
        "face id {id} already exists in the graph (violates /req/core/topology-faceID, §7.13.5.3)"
    )]
    DuplicateFaceId { id: FaceId },
    /// Requirement Topo5 (§7.13.4.5): a directed edge SHALL include the
    /// IDs of existing start and end nodes.
    #[error(
        "node id {id} is not in the graph; a directed edge SHALL include the start and end node IDs (violates /req/core/topology-edge-dir, §7.13.4.5)"
    )]
    UnknownNodeId { id: NodeId },
    /// A referenced edge (clip target or face-boundary step) is absent
    /// from the graph (§7.13.4.7 / §7.13.5.4).
    #[error("edge id {id} is not in the graph")]
    UnknownEdgeId { id: EdgeId },
    /// Operational precondition of Requirement Topo6 (§7.13.4.7, not its
    /// own spec box): a clip needs a polyline — the edge has no geometry
    /// and its endpoint nodes have no positions.
    #[error(
        "edge {id} has no derivable polyline (no geometry, endpoint nodes without positions); cannot clip (precondition of /req/core/topology-clip, table slug -edge-clip, §7.13.4.7)"
    )]
    EdgeHasNoGeometry { id: EdgeId },
    /// Operational precondition of Requirement Topo6 (§7.13.4.7): where
    /// geometry and endpoint node positions both exist they must agree
    /// bit-exactly; a mismatch is detached data no clip can split
    /// coherently.
    #[error(
        "edge {id}'s geometry endpoints do not coincide with its start/end node positions; refusing to clip detached data (precondition of /req/core/topology-clip, §7.13.4.7)"
    )]
    EdgeGeometryEndpointMismatch { id: EdgeId },
    /// Operational precondition of Requirement Topo6 (§7.13.4.7): every
    /// polyline coordinate must be finite — NaN/infinite positions would
    /// classify as "outside" and mint non-finite clip nodes.
    #[error(
        "edge {id}'s polyline contains a non-finite coordinate; cannot clip (precondition of /req/core/topology-clip, §7.13.4.7)"
    )]
    EdgeGeometryNotFinite { id: EdgeId },
    /// Operational precondition of Requirement Topo6 (§7.13.4.7): a tile
    /// extent needs `west < east` and `south < north` (degenerate and
    /// antimeridian-crossing boxes rejected; neither shipped grid
    /// produces one).
    #[error(
        "clip extent west={west} south={south} east={east} north={north} is degenerate or antimeridian-crossing (precondition of /req/core/topology-clip, §7.13.4.7)"
    )]
    InvalidClipExtent {
        west: f64,
        south: f64,
        east: f64,
        north: f64,
    },
    /// Requirement Topo6 (§7.13.4.7) is titled "Clipping edges":
    /// face/polygon clipping is an application-profile concern, and
    /// silently invalidating a face's boundary is unacceptable.
    #[error(
        "edge {edge} bounds face {face}; §7.13.4.7 specifies clipping of edges only — face clipping is an application-profile concern"
    )]
    EdgeInFace { edge: EdgeId, face: FaceId },
    /// Face Topology Requirement 3 (§7.13.5.4): an exterior boundary is
    /// a non-empty ring.
    #[error(
        "face {face} has an empty boundary (violates /req/core/topology-face-structure, box slug /rec/…, §7.13.5.4)"
    )]
    FaceBoundaryEmpty { face: FaceId },
    /// Face Topology Requirement 3 (§7.13.5.4): consecutive directed
    /// edges must chain — "exterior boundary (aka polygon)" entails it
    /// (meaning-keyed reading). `step` is the 0-based boundary step whose
    /// to-node differs from the next step's from-node.
    #[error(
        "face {face} boundary step {step} does not chain to the next step (violates /req/core/topology-face-structure, §7.13.5.4)"
    )]
    FaceBoundaryNotChained { face: FaceId, step: usize },
    /// Face Topology Requirement 3 (§7.13.5.4): the ring must close —
    /// the last step's to-node is the first step's from-node.
    #[error(
        "face {face} boundary does not close back to its first node (violates /req/core/topology-face-structure, §7.13.5.4)"
    )]
    FaceBoundaryNotClosed { face: FaceId },
    /// Face Topology Requirement 4 (§7.13.5.5): winding order is
    /// clockwise or counterclockwise.
    #[error(
        "winding order {value:?} is not clockwise|counterclockwise (violates /req/core/topology-winding, box slug -face-winding, §7.13.5.5)"
    )]
    UnknownWindingOrder { value: String },
    /// Face Topology Requirement 4 (§7.13.5.5): once faces are
    /// generated, the winding order SHALL be documented in the dataset's
    /// metadata.
    #[error(
        "dataset contains faces but its resource metadata declares no windingOrder (violates /req/core/topology-winding, box slug -face-winding, §7.13.5.5)"
    )]
    WindingOrderUndeclared,
    /// The dataset's resource metadata failed its own module's
    /// validation; surfaced through the topology family so a dataset
    /// check yields a single error type (Face4 delegates to Metadata).
    #[error(transparent)]
    Metadata(#[from] MetadataViolation),
}

/// What [`TopoGraph::clip_edge_to_tile`] did (Requirement Topo6,
/// §7.13.4.7). Part edge IDs appear in traversal order; `clip_nodes`
/// holds one minted node per boundary crossing, also in traversal
/// order. When `clip_nodes` is empty the edge did not cross the
/// boundary and is untouched, reported whole in the matching bucket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeClipOutcome {
    /// Parts inside the closed tile extent.
    pub inside: Vec<EdgeId>,
    /// Parts outside the tile extent.
    pub outside: Vec<EdgeId>,
    /// The artificial (virtual) nodes minted at boundary crossings.
    pub clip_nodes: Vec<NodeId>,
}

/// An alternating containment run of the clip walk: `(inside?, coords)`.
type Run = (bool, Vec<Coord<f64>>);

/// The tile extent as a closed `geo` rectangle. `Rect::new` normalizes
/// min/max; the extent was already validated `west < east && south <
/// north`. `Rect: Intersects<Coord>` is exactly closed containment
/// (`>= min && <= max`), the semantics Topo6's touch/crossing
/// distinction is built on.
fn tile_rect(tile: &Bbox) -> Rect<f64> {
    Rect::new(
        coord! { x: tile.west, y: tile.south },
        coord! { x: tile.east, y: tile.north },
    )
}

/// Appends `c` unless the run already ends with it — crossings that
/// coincide with polyline vertices would otherwise duplicate
/// coordinates.
fn push_coord(run: &mut Vec<Coord<f64>>, c: Coord<f64>) {
    if run.last() != Some(&c) {
        run.push(c);
    }
}

/// The Liang–Barsky slab interval of segment a→b against the closed
/// tile: parameters `enter ≤ exit` (relative to [0, 1]) plus which
/// boundary coordinate is pinned at each end (both at a corner). `None`
/// when the segment misses the tile entirely.
struct SlabHit {
    enter: f64,
    exit: f64,
    enter_x_pin: Option<f64>,
    enter_y_pin: Option<f64>,
    exit_x_pin: Option<f64>,
    exit_y_pin: Option<f64>,
}

fn slab_interval(tile: &Bbox, a: Coord<f64>, b: Coord<f64>) -> Option<SlabHit> {
    let mut hit = SlabHit {
        enter: 0.0,
        exit: 1.0,
        enter_x_pin: None,
        enter_y_pin: None,
        exit_x_pin: None,
        exit_y_pin: None,
    };
    let dx = b.x - a.x;
    if dx == 0.0 {
        if a.x < tile.west || a.x > tile.east {
            return None;
        }
    } else {
        let (t_near, near_pin, t_far, far_pin) = if dx > 0.0 {
            (
                (tile.west - a.x) / dx,
                tile.west,
                (tile.east - a.x) / dx,
                tile.east,
            )
        } else {
            (
                (tile.east - a.x) / dx,
                tile.east,
                (tile.west - a.x) / dx,
                tile.west,
            )
        };
        if t_near > hit.enter {
            hit.enter = t_near;
            hit.enter_x_pin = Some(near_pin);
            hit.enter_y_pin = None;
        } else if t_near == hit.enter {
            hit.enter_x_pin = Some(near_pin);
        }
        if t_far < hit.exit {
            hit.exit = t_far;
            hit.exit_x_pin = Some(far_pin);
            hit.exit_y_pin = None;
        } else if t_far == hit.exit {
            hit.exit_x_pin = Some(far_pin);
        }
    }
    let dy = b.y - a.y;
    if dy == 0.0 {
        if a.y < tile.south || a.y > tile.north {
            return None;
        }
    } else {
        let (t_near, near_pin, t_far, far_pin) = if dy > 0.0 {
            (
                (tile.south - a.y) / dy,
                tile.south,
                (tile.north - a.y) / dy,
                tile.north,
            )
        } else {
            (
                (tile.north - a.y) / dy,
                tile.north,
                (tile.south - a.y) / dy,
                tile.south,
            )
        };
        if t_near > hit.enter {
            hit.enter = t_near;
            hit.enter_y_pin = Some(near_pin);
            hit.enter_x_pin = None;
        } else if t_near == hit.enter {
            hit.enter_y_pin = Some(near_pin);
        }
        if t_far < hit.exit {
            hit.exit = t_far;
            hit.exit_y_pin = Some(far_pin);
            hit.exit_x_pin = None;
        } else if t_far == hit.exit {
            hit.exit_y_pin = Some(far_pin);
        }
    }
    (hit.enter <= hit.exit).then_some(hit)
}

/// The crossing coordinate at parameter `t` along a→b, with any pinned
/// boundary coordinate substituted *exactly* — the property Requirement
/// Topo6's shared-identifier rule turns on: both grids' tile extents are
/// dyadic rationals, adjacent tiles agree on the shared boundary
/// bitwise, and pinning reproduces that exact value in the minted node.
/// The unpinned axis is interpolated endpoint-exactly: at `t == 0`/`1`
/// the crossing *is* the segment endpoint and is returned bitwise rather
/// than recomputed, since an `a + 1.0·(b − a)` miss of ~1 ulp would mint
/// an out-of-tile node and split a boundary touch into phantom parts.
fn point_at(
    a: Coord<f64>,
    b: Coord<f64>,
    t: f64,
    x_pin: Option<f64>,
    y_pin: Option<f64>,
) -> Coord<f64> {
    // Endpoint-exact lerp: at t == 0/1 the crossing IS the segment
    // endpoint, and the interpolation formula must reproduce it bitwise —
    // `a + 1.0·(b − a)` generally does not, and a 1-ulp miss here turns a
    // boundary touch into a phantom split with an out-of-tile node.
    let lerp = |pa: f64, pb: f64| {
        if t == 0.0 {
            pa
        } else if t == 1.0 {
            pb
        } else {
            pa + t * (pb - pa)
        }
    };
    Coord {
        x: x_pin.unwrap_or_else(|| lerp(a.x, b.x)),
        y: y_pin.unwrap_or_else(|| lerp(a.y, b.y)),
    }
}

/// A topologically structured vector dataset: the edge-node(-face) graph
/// of §7.13.1, enforcing the module's SHALLs at insert time — duplicate
/// identifiers are unrepresentable (Topo2/Topo3/Face2), every edge's
/// endpoints must exist (Topo5), and the signed directed-node adjacency
/// (Topo4) is maintained by the graph itself, so a wrong sign cannot be
/// constructed. The only mutations are the `insert_*` methods and
/// [`TopoGraph::clip_edge_to_tile`]; there is no public delete API.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TopoGraph {
    nodes: BTreeMap<NodeId, TopoNode>,
    edges: BTreeMap<EdgeId, TopoEdge>,
    faces: BTreeMap<FaceId, TopoFace>,
    /// The directed-node view (Topo4): per node, incident edges marked
    /// [`SignedEdge::Leaving`] ("−") / [`SignedEdge::Entering`] ("+") in
    /// insertion order.
    adjacency: BTreeMap<NodeId, Vec<SignedEdge>>,
    /// Monotone mint counters (max inserted id + 1) for the clip
    /// operation's artificial nodes and part edges (Topo5's "artificial
    /// (virtual) nodes generated by processes that clip edges").
    next_node_id: u64,
    next_edge_id: u64,
}

/// The wire form of a [`TopoGraph`]: its three primitive collections and
/// nothing else. The Topo4 adjacency and the clip mint counters are
/// *derived* from those collections, so putting them on the wire would only
/// create a second copy that could arrive disagreeing with the first.
///
/// `deny_unknown_fields` makes that stance enforceable rather than merely
/// stated: a document carrying an `adjacency` or a `next_node_id` is
/// **rejected**, not quietly stripped. Accepting it would let a writer
/// believe the crate honoured a view it in fact discarded — the same
/// disagreement the exclusion exists to prevent, arriving by the back door.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TopoGraphWire {
    nodes: Vec<TopoNode>,
    edges: Vec<TopoEdge>,
    faces: Vec<TopoFace>,
}

/// Serializes as `{ nodes, edges, faces }`, each in identifier order
/// (the graph stores them in `BTreeMap`s, so the bytes are deterministic).
/// Wanted since Phase 11, so a vector tile can carry its topology.
impl Serialize for TopoGraph {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let mut graph = serializer.serialize_struct("TopoGraph", 3)?;
        graph.serialize_field("nodes", &self.nodes.values().collect::<Vec<_>>())?;
        graph.serialize_field("edges", &self.edges.values().collect::<Vec<_>>())?;
        graph.serialize_field("faces", &self.faces.values().collect::<Vec<_>>())?;
        graph.end()
    }
}

/// Rebuilds the graph through [`TopoGraph::insert_node`],
/// [`TopoGraph::insert_edge`] and [`TopoGraph::insert_face`] rather than
/// populating the fields directly, so a graph that arrived over the wire
/// satisfies the module's SHALLs exactly as one built in memory does:
/// unique identifiers (Topo2/Topo3/Face2), existing edge endpoints (Topo5),
/// a chained and closed face boundary (Face3), and a directed-node adjacency
/// (Topo4) that is regenerated rather than trusted. A wire graph that breaks
/// one is a deserialization error carrying that violation's message — the
/// alternative, a `#[derive]` onto the private fields, would mint graphs no
/// constructor could have produced.
///
/// Ordering is not a wire concern: nodes are inserted before the edges that
/// reference them and edges before the faces that bound them, whatever order
/// the document lists the three collections in.
impl<'de> Deserialize<'de> for TopoGraph {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = TopoGraphWire::deserialize(deserializer)?;
        let mut graph = TopoGraph::new();
        for node in wire.nodes {
            graph.insert_node(node).map_err(serde::de::Error::custom)?;
        }
        for edge in wire.edges {
            graph.insert_edge(edge).map_err(serde::de::Error::custom)?;
        }
        for face in wire.faces {
            graph.insert_face(face).map_err(serde::de::Error::custom)?;
        }
        Ok(graph)
    }
}

impl TopoGraph {
    /// An empty graph.
    pub fn new() -> TopoGraph {
        TopoGraph::default()
    }

    /// Inserts a node (Requirement Topo2, §7.13.4.2): the identifier
    /// must be unique.
    pub fn insert_node(&mut self, node: TopoNode) -> Result<(), TopologyViolation> {
        if self.nodes.contains_key(&node.id) {
            return Err(TopologyViolation::DuplicateNodeId { id: node.id });
        }
        self.next_node_id = self.next_node_id.max(node.id.0.saturating_add(1));
        self.nodes.insert(node.id, node);
        Ok(())
    }

    /// Inserts an edge (Requirements Topo3/Topo5, §7.13.4.3/§7.13.4.5):
    /// the identifier must be unique and both endpoint nodes must exist.
    /// On success the endpoints' directed views gain the Topo4 markings —
    /// `Leaving` on the start node, `Entering` on the end node (both on
    /// the single node of a loop edge).
    pub fn insert_edge(&mut self, edge: TopoEdge) -> Result<(), TopologyViolation> {
        if self.edges.contains_key(&edge.id) {
            return Err(TopologyViolation::DuplicateEdgeId { id: edge.id });
        }
        for endpoint in [edge.start, edge.end] {
            if !self.nodes.contains_key(&endpoint) {
                return Err(TopologyViolation::UnknownNodeId { id: endpoint });
            }
        }
        self.adjacency
            .entry(edge.start)
            .or_default()
            .push(SignedEdge::Leaving(edge.id));
        self.adjacency
            .entry(edge.end)
            .or_default()
            .push(SignedEdge::Entering(edge.id));
        self.next_edge_id = self.next_edge_id.max(edge.id.0.saturating_add(1));
        self.edges.insert(edge.id, edge);
        Ok(())
    }

    /// Inserts a face (Face Topology Requirements 2 and 3,
    /// §7.13.5.3/§7.13.5.4): the identifier must be unique, every
    /// boundary edge must exist, and the directed edges must form a
    /// chained, closed, non-empty exterior ring — "exterior boundary
    /// (aka polygon)" entails chaining and closure (meaning-keyed
    /// reading, module docs).
    pub fn insert_face(&mut self, face: TopoFace) -> Result<(), TopologyViolation> {
        if self.faces.contains_key(&face.id) {
            return Err(TopologyViolation::DuplicateFaceId { id: face.id });
        }
        if face.boundary.is_empty() {
            return Err(TopologyViolation::FaceBoundaryEmpty { face: face.id });
        }
        let mut hops = Vec::with_capacity(face.boundary.len());
        for step in &face.boundary {
            let edge = self
                .edges
                .get(&step.edge)
                .ok_or(TopologyViolation::UnknownEdgeId { id: step.edge })?;
            hops.push(step.endpoints(edge));
        }
        for (step, pair) in hops.windows(2).enumerate() {
            let &[(_, to), (from, _)] = pair else {
                continue;
            };
            if to != from {
                return Err(TopologyViolation::FaceBoundaryNotChained {
                    face: face.id,
                    step,
                });
            }
        }
        match (hops.first(), hops.last()) {
            (Some(&(first_from, _)), Some(&(_, last_to))) if last_to == first_from => {}
            (Some(_), Some(_)) => {
                return Err(TopologyViolation::FaceBoundaryNotClosed { face: face.id });
            }
            _ => return Err(TopologyViolation::FaceBoundaryEmpty { face: face.id }),
        }
        self.faces.insert(face.id, face);
        Ok(())
    }

    /// The face with `id`, if present.
    pub fn face(&self, id: FaceId) -> Option<&TopoFace> {
        self.faces.get(&id)
    }

    /// All faces in ascending id order.
    pub fn faces(&self) -> impl Iterator<Item = &TopoFace> {
        self.faces.values()
    }

    /// Number of faces.
    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    /// The node with `id`, if present.
    pub fn node(&self, id: NodeId) -> Option<&TopoNode> {
        self.nodes.get(&id)
    }

    /// The edge with `id`, if present.
    pub fn edge(&self, id: EdgeId) -> Option<&TopoEdge> {
        self.edges.get(&id)
    }

    /// The directed-node view of `id` (Requirement Topo4, §7.13.4.4):
    /// empty for an isolated *or absent* node — Topo4's lead-in exempts
    /// isolated nodes, and absence is always caught at insert time, so
    /// no validation flow needs the distinction.
    pub fn directed_edges(&self, id: NodeId) -> &[SignedEdge] {
        self.adjacency.get(&id).map(Vec::as_slice).unwrap_or(&[])
    }

    /// All nodes in ascending id order.
    pub fn nodes(&self) -> impl Iterator<Item = &TopoNode> {
        self.nodes.values()
    }

    /// All edges in ascending id order.
    pub fn edges(&self) -> impl Iterator<Item = &TopoEdge> {
        self.edges.values()
    }

    /// Number of nodes.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Number of edges.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Derives the polyline the clip operates on: the edge's own
    /// geometry, else the straight segment between its endpoint node
    /// positions (§7.13.1's "connected with no geometry" case). Where
    /// geometry and node positions both exist they must agree
    /// bit-exactly at the endpoints — this module's own minting keeps
    /// them exact, so a mismatch means detached data no clip can split
    /// coherently.
    fn edge_polyline(&self, edge: &TopoEdge) -> Result<Vec<Coord<f64>>, TopologyViolation> {
        let start_position = self.nodes.get(&edge.start).and_then(|node| node.position);
        let end_position = self.nodes.get(&edge.end).and_then(|node| node.position);
        let coords: Vec<Coord<f64>> = match &edge.geometry {
            Some(line) => {
                let coords: Vec<Coord<f64>> = line.coords().copied().collect();
                let (Some(&first), Some(&last)) = (coords.first(), coords.last()) else {
                    return Err(TopologyViolation::EdgeHasNoGeometry { id: edge.id });
                };
                if coords.len() < 2 {
                    return Err(TopologyViolation::EdgeHasNoGeometry { id: edge.id });
                }
                if let Some(position) = start_position
                    && (position.x(), position.y()) != (first.x, first.y)
                {
                    return Err(TopologyViolation::EdgeGeometryEndpointMismatch { id: edge.id });
                }
                if let Some(position) = end_position
                    && (position.x(), position.y()) != (last.x, last.y)
                {
                    return Err(TopologyViolation::EdgeGeometryEndpointMismatch { id: edge.id });
                }
                coords
            }
            None => match (start_position, end_position) {
                (Some(start), Some(end)) => vec![
                    Coord {
                        x: start.x(),
                        y: start.y(),
                    },
                    Coord {
                        x: end.x(),
                        y: end.y(),
                    },
                ],
                _ => return Err(TopologyViolation::EdgeHasNoGeometry { id: edge.id }),
            },
        };
        // Non-finite coordinates (NaN/±∞) are a clip precondition failure:
        // classification would treat them as "outside" and the walk would
        // mint non-finite nodes. Reject once, on the assembled polyline.
        if coords.iter().any(|c| !c.x.is_finite() || !c.y.is_finite()) {
            return Err(TopologyViolation::EdgeGeometryNotFinite { id: edge.id });
        }
        Ok(coords)
    }

    /// Clips `edge_id` against a tile extent (Requirement Topo6,
    /// /req/core/topology-clip — the class table's slug is `-edge-clip`
    /// — §7.13.4.7).
    ///
    /// Containment is *closed* (boundary points are inside), so an
    /// endpoint or vertex exactly on a boundary or corner is a touch,
    /// not a crossing, and a run collinear along the boundary stays
    /// inside — the §7.13.4.7 NOTE's corner cases, decided here and
    /// tested. Each inside↔outside transition mints one artificial node
    /// whose crossing has the boundary coordinate pinned exactly; the
    /// edge is replaced by its parts (fresh IDs, sub-polylines chaining
    /// original start → clip nodes → original end), keeping Topo5 for
    /// every part and updating Topo4 adjacency. Because both shipped
    /// grids' extents are dyadic and adjacent tiles agree bitwise on the
    /// shared boundary, the minted node lies on the neighbour tile's
    /// closed boundary too: clipping the outside parts there mints
    /// nothing new — one NodeId shared by both tiles, answering the
    /// NOTE's precision concern.
    ///
    /// No crossing → the graph is untouched and the outcome reports the
    /// edge whole in its bucket. Errors (all pre-mutation): unknown
    /// edge; [`TopologyViolation::EdgeInFace`] (§7.13.4.7 clips *edges*
    /// only); [`TopologyViolation::EdgeHasNoGeometry`],
    /// [`TopologyViolation::EdgeGeometryEndpointMismatch`],
    /// [`TopologyViolation::EdgeGeometryNotFinite`],
    /// [`TopologyViolation::InvalidClipExtent`] (operational
    /// preconditions). Ids at the very top of the `u64` range are refused
    /// before any mutation (the mint counters must not saturate mid-clip).
    pub fn clip_edge_to_tile(
        &mut self,
        edge_id: EdgeId,
        tile: Bbox,
    ) -> Result<EdgeClipOutcome, TopologyViolation> {
        if !(tile.west < tile.east && tile.south < tile.north) {
            return Err(TopologyViolation::InvalidClipExtent {
                west: tile.west,
                south: tile.south,
                east: tile.east,
                north: tile.north,
            });
        }
        let edge = self
            .edges
            .get(&edge_id)
            .ok_or(TopologyViolation::UnknownEdgeId { id: edge_id })?;
        if let Some(face) = self
            .faces
            .values()
            .find(|face| face.boundary.iter().any(|step| step.edge == edge_id))
        {
            return Err(TopologyViolation::EdgeInFace {
                edge: edge_id,
                face: face.id,
            });
        }
        let (orig_start, orig_end) = (edge.start, edge.end);
        let pts = self.edge_polyline(edge)?;

        let rect = tile_rect(&tile);
        let Some(&first_pt) = pts.first() else {
            return Err(TopologyViolation::EdgeHasNoGeometry { id: edge_id });
        };
        let mut state = rect.intersects(&first_pt);
        let mut runs: Vec<Run> = Vec::new();
        let mut crossings: Vec<Coord<f64>> = Vec::new();
        let mut current = vec![first_pt];
        for pair in pts.windows(2) {
            let &[a, b] = pair else { continue };
            let a_in = rect.intersects(&a);
            let b_in = rect.intersects(&b);
            debug_assert_eq!(a_in, state, "walk state desynced from containment");
            match (a_in, b_in) {
                (true, true) => push_coord(&mut current, b),
                (true, false) => {
                    // a lies in the closed box, so the segment exits at
                    // the interval's far end (t = 0 when a is already on
                    // the boundary). The total fallback cannot fire but
                    // keeps the arithmetic panic-free.
                    let c = match slab_interval(&tile, a, b) {
                        Some(hit) => point_at(a, b, hit.exit, hit.exit_x_pin, hit.exit_y_pin),
                        None => a,
                    };
                    push_coord(&mut current, c);
                    runs.push((true, std::mem::take(&mut current)));
                    crossings.push(c);
                    push_coord(&mut current, c);
                    push_coord(&mut current, b);
                    state = false;
                }
                (false, true) => {
                    let c = match slab_interval(&tile, a, b) {
                        Some(hit) => point_at(a, b, hit.enter, hit.enter_x_pin, hit.enter_y_pin),
                        None => b,
                    };
                    push_coord(&mut current, c);
                    runs.push((false, std::mem::take(&mut current)));
                    crossings.push(c);
                    push_coord(&mut current, c);
                    push_coord(&mut current, b);
                    state = true;
                }
                (false, false) => {
                    let through = slab_interval(&tile, a, b).and_then(|hit| {
                        if hit.enter < hit.exit {
                            let enter = point_at(a, b, hit.enter, hit.enter_x_pin, hit.enter_y_pin);
                            let exit = point_at(a, b, hit.exit, hit.exit_x_pin, hit.exit_y_pin);
                            (enter != exit).then_some((enter, exit))
                        } else {
                            None
                        }
                    });
                    if let Some((enter, exit)) = through {
                        push_coord(&mut current, enter);
                        runs.push((false, std::mem::take(&mut current)));
                        crossings.push(enter);
                        runs.push((true, vec![enter, exit]));
                        crossings.push(exit);
                        push_coord(&mut current, exit);
                        push_coord(&mut current, b);
                    } else {
                        // Miss, or a single-point graze of the boundary:
                        // a touch is not a crossing.
                        push_coord(&mut current, b);
                    }
                }
            }
        }
        runs.push((state, current));

        // Touches are not crossings: a run holding fewer than two
        // coordinates is a boundary touch (the polyline starts or ends
        // on the boundary, or an outside polyline meets it at exactly
        // one vertex). Drop it, cancel its crossings, and merge its
        // equal-state neighbours.
        while let Some(pos) = runs.iter().position(|(_, coords)| coords.len() < 2) {
            runs.remove(pos);
            if pos == 0 {
                if !crossings.is_empty() {
                    crossings.remove(0);
                }
            } else if pos == runs.len() {
                crossings.pop();
            } else {
                crossings.remove(pos);
                crossings.remove(pos - 1);
                let (right_state, right_coords) = runs.remove(pos);
                if let Some((left_state, left_coords)) = runs.get_mut(pos - 1) {
                    debug_assert_eq!(
                        *left_state, right_state,
                        "merged neighbours must share containment state"
                    );
                    for coord in right_coords {
                        push_coord(left_coords, coord);
                    }
                }
            }
        }
        debug_assert_eq!(crossings.len(), runs.len().saturating_sub(1));

        if runs.len() < 2 {
            let inside = runs
                .first()
                .map(|(state, _)| *state)
                .unwrap_or_else(|| rect.intersects(&first_pt));
            return Ok(EdgeClipOutcome {
                inside: if inside { vec![edge_id] } else { Vec::new() },
                outside: if inside { Vec::new() } else { vec![edge_id] },
                clip_nodes: Vec::new(),
            });
        }

        // Pre-mutation headroom guard: minting must not run the id
        // counters into u64::MAX (a graph holding ids that high — the
        // saturated counter case — would collide mid-mint). Conservative
        // by exactly one id at the top of the range, documented.
        if self
            .next_node_id
            .checked_add(crossings.len() as u64)
            .is_none()
        {
            return Err(TopologyViolation::DuplicateNodeId {
                id: NodeId(u64::MAX),
            });
        }
        if self.next_edge_id.checked_add(runs.len() as u64).is_none() {
            return Err(TopologyViolation::DuplicateEdgeId {
                id: EdgeId(u64::MAX),
            });
        }

        // Replace the edge with its parts. Minted identifiers are fresh
        // by construction (monotone counters), so these inserts cannot
        // fail; `?` keeps the flow total without unwrap.
        self.edges.remove(&edge_id);
        for endpoint in [orig_start, orig_end] {
            if let Some(adjacent) = self.adjacency.get_mut(&endpoint) {
                adjacent.retain(|signed| signed.edge() != edge_id);
            }
        }
        let mut clip_nodes = Vec::with_capacity(crossings.len());
        for crossing in &crossings {
            let id = NodeId(self.next_node_id);
            self.insert_node(TopoNode {
                id,
                position: Some(geo_types::Point::new(crossing.x, crossing.y)),
            })?;
            clip_nodes.push(id);
        }
        let mut junctions = Vec::with_capacity(runs.len() + 1);
        junctions.push(orig_start);
        junctions.extend(clip_nodes.iter().copied());
        junctions.push(orig_end);
        let mut inside = Vec::new();
        let mut outside = Vec::new();
        for ((run_inside, coords), pair) in runs.into_iter().zip(junctions.windows(2)) {
            let &[start, end] = pair else { continue };
            let id = EdgeId(self.next_edge_id);
            self.insert_edge(TopoEdge {
                id,
                start,
                end,
                geometry: Some(geo_types::LineString::from(coords)),
            })?;
            if run_inside {
                inside.push(id);
            } else {
                outside.push(id);
            }
        }
        Ok(EdgeClipOutcome {
            inside,
            outside,
            clip_nodes,
        })
    }
}

/// Validates a topologically structured dataset against its resource
/// metadata record.
///
/// 1. The record is validated by its own module (delegated through
///    [`TopologyViolation::Metadata`], the same shape as
///    [`crate::coverage::validate_coverage_instance`]).
/// 2. Face Topology Requirement 4 (/req/core/topology-winding; box slug
///    `-face-winding`, §7.13.5.5): once the graph contains faces, the
///    record SHALL declare a `windingOrder`, else
///    [`TopologyViolation::WindingOrderUndeclared`]. A declared winding
///    with no faces is harmless; a face-less graph needs none.
pub fn validate_topology_dataset(
    graph: &TopoGraph,
    record: &ResourceMetadata,
) -> Result<(), TopologyViolation> {
    record.validate()?;
    if graph.face_count() > 0 && record.winding_order.is_none() {
        return Err(TopologyViolation::WindingOrderUndeclared);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §7.13.1 — the topological primitives round-trip through serde, wanted
    /// since Phase 11 so a vector tile can carry its edge-node-face graph.
    /// The identifiers are transparent integers on the wire, and a
    /// [`TopoGraph`] serializes as exactly its three primitive collections:
    /// the Topo4 adjacency and the clip mint counters are **derived**, so
    /// they are not on the wire and cannot arrive there disagreeing with the
    /// primitives.
    #[test]
    fn req_core_topology_primitives_serde_round_trip() {
        let mut graph = TopoGraph::new();
        graph
            .insert_node(TopoNode {
                id: NodeId(1),
                position: Some(geo_types::Point::new(1.0, 2.0)),
            })
            .unwrap();
        graph
            .insert_node(TopoNode {
                id: NodeId(2),
                position: None,
            })
            .unwrap();
        graph
            .insert_edge(TopoEdge {
                id: EdgeId(7),
                start: NodeId(1),
                end: NodeId(2),
                geometry: Some(geo_types::LineString::from(vec![(1.0, 2.0), (3.0, 4.0)])),
            })
            .unwrap();
        graph
            .insert_edge(TopoEdge {
                id: EdgeId(8),
                start: NodeId(2),
                end: NodeId(1),
                geometry: None,
            })
            .unwrap();
        graph
            .insert_face(TopoFace {
                id: FaceId(3),
                boundary: vec![
                    DirectedEdge {
                        edge: EdgeId(7),
                        orientation: Orientation::Forward,
                    },
                    DirectedEdge {
                        edge: EdgeId(8),
                        orientation: Orientation::Forward,
                    },
                ],
            })
            .unwrap();

        // The struct carries exactly the three primitive collections: no
        // adjacency, no mint counters.
        let text = serde_json::to_string(&graph).unwrap();
        assert!(text.starts_with(r#"{"nodes":"#), "{text}");
        let value = serde_json::to_value(&graph).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["edges", "faces", "nodes"]);
        assert_eq!(value["nodes"][0]["id"], 1);
        assert_eq!(value["edges"][0]["id"], 7);
        assert_eq!(value["edges"][0]["start"], 1);
        assert_eq!(value["faces"][0]["boundary"][0]["edge"], 7);
        assert_eq!(value["faces"][0]["boundary"][0]["orientation"], "forward");

        // The two fields this crate does not own. `position` and `geometry`
        // are serialized entirely by `geo-types`' own derives, in its native
        // `{"x":…,"y":…}` coordinate form — not GeoJSON, not WKT. That shape
        // is part of this crate's frozen 1.0 wire surface, so it is pinned
        // here rather than left to a dependency's discretion: a `geo-types`
        // release that changed `Coord`'s representation would otherwise
        // invalidate every persisted topology document silently.
        assert_eq!(
            value["nodes"][0]["position"],
            serde_json::json!({"x": 1.0, "y": 2.0})
        );
        assert_eq!(value["nodes"][1]["position"], serde_json::Value::Null);
        assert_eq!(
            value["edges"][0]["geometry"],
            serde_json::json!([{"x": 1.0, "y": 2.0}, {"x": 3.0, "y": 4.0}])
        );
        assert_eq!(value["edges"][1]["geometry"], serde_json::Value::Null);

        let back: TopoGraph = serde_json::from_value(value).unwrap();
        assert_eq!(back, graph);
        // The derived Topo4 adjacency was rebuilt, not transported.
        assert_eq!(
            back.directed_edges(NodeId(1)),
            [
                SignedEdge::Leaving(EdgeId(7)),
                SignedEdge::Entering(EdgeId(8))
            ]
        );
    }

    /// §7.13.4.2–§7.13.5.4 — deserializing a graph goes through the same
    /// `insert_*` path a caller uses, so the module's SHALL invariants hold
    /// for a graph that arrived over the wire exactly as for one built in
    /// memory: duplicate identifiers (Topo2/Topo3/Face2), edges whose
    /// endpoint nodes do not exist (Topo5), and unchained face boundaries
    /// (Face3) are all rejected at parse time rather than producing a graph
    /// no constructor could have made.
    #[test]
    fn req_core_topology_graph_deserialize_enforces_invariants() {
        let duplicate = r#"{"nodes":[{"id":1,"position":null},{"id":1,"position":null}],
                            "edges":[],"faces":[]}"#;
        let error = serde_json::from_str::<TopoGraph>(duplicate).unwrap_err();
        assert!(error.to_string().contains("node id 1"), "{error}");

        let dangling = r#"{"nodes":[{"id":1,"position":null}],
                           "edges":[{"id":1,"start":1,"end":9,"geometry":null}],"faces":[]}"#;
        let error = serde_json::from_str::<TopoGraph>(dangling).unwrap_err();
        assert!(error.to_string().contains("node id 9"), "{error}");

        let unclosed = r#"{"nodes":[{"id":1,"position":null},{"id":2,"position":null}],
                           "edges":[{"id":1,"start":1,"end":2,"geometry":null}],
                           "faces":[{"id":1,"boundary":[{"edge":1,"orientation":"forward"}]}]}"#;
        let error = serde_json::from_str::<TopoGraph>(unclosed).unwrap_err();
        assert!(error.to_string().contains("close"), "{error}");

        // The derived views are deliberately absent from the wire form
        // because a transported copy could arrive disagreeing with the
        // collections it is derived from. A document that carries one anyway
        // is therefore *rejected*, not silently ignored: accepting it would
        // let a writer believe the crate honoured an adjacency or a mint
        // counter it in fact discarded.
        let derived = r#"{"nodes":[],"edges":[],"faces":[],"adjacency":{"1":[]}}"#;
        let error = serde_json::from_str::<TopoGraph>(derived).unwrap_err();
        assert!(error.to_string().contains("adjacency"), "{error}");
        let counter = r#"{"nodes":[],"edges":[],"faces":[],"next_node_id":99}"#;
        let error = serde_json::from_str::<TopoGraph>(counter).unwrap_err();
        assert!(error.to_string().contains("next_node_id"), "{error}");
    }

    /// Face Topology Requirement 4 (/req/core/topology-winding, §7.13.5.5) —
    /// [`WindingOrder`] serializes as the wire spelling it carries on the
    /// `windingOrder` metadata element, not as a Rust variant name, so a
    /// serialized graph and a metadata record agree letter for letter.
    #[test]
    fn req_core_topology_winding_serde_uses_wire_spelling() {
        for winding in WindingOrder::ALL {
            let json = serde_json::to_string(&winding).unwrap();
            assert_eq!(json, format!("\"{}\"", winding.as_str()));
            assert_eq!(
                serde_json::from_str::<WindingOrder>(&json).unwrap(),
                winding
            );
        }
        assert!(serde_json::from_str::<WindingOrder>("\"Clockwise\"").is_err());
    }

    /// Requirement Topo1 /req/core/topology-topic1 (§7.13.4.1; the box's
    /// "ISO 19101:2017" is a draft typo for ISO 19107:2017) — topological
    /// primitives are their own types, equivalent to but not subclasses
    /// of geometric primitives: a pure topology graph carries no
    /// coordinates, so every primitive is constructible coordinate-free.
    #[test]
    fn req_core_topology_topic1_primitives_are_coordinate_free() {
        let node = TopoNode {
            id: NodeId(1),
            position: None,
        };
        let edge = TopoEdge {
            id: EdgeId(1),
            start: NodeId(1),
            end: NodeId(1),
            geometry: None,
        };
        let face = TopoFace {
            id: FaceId(1),
            boundary: vec![DirectedEdge {
                edge: EdgeId(1),
                orientation: Orientation::Forward,
            }],
        };
        assert_eq!(node.position, None);
        assert_eq!(edge.geometry, None);
        assert_eq!(face.boundary.len(), 1);
    }

    /// Requirement Topo4 /req/core/topology-node-dir (§7.13.4.4) and the
    /// ISO 19107 directed-node definition (§7.13.1) — the signed markings
    /// render exactly as the spec writes them: "-" leaving/start, "+"
    /// entering/end.
    #[test]
    fn req_core_topology_node_dir_sign_rendering() {
        assert_eq!(SignedEdge::Leaving(EdgeId(17)).to_string(), "-17");
        assert_eq!(SignedEdge::Entering(EdgeId(17)).to_string(), "+17");
        assert_eq!(SignedEdge::Leaving(EdgeId(17)).edge(), EdgeId(17));
        let start = DirectedNode {
            node: NodeId(5),
            sign: NodeSign::Start,
        };
        let end = DirectedNode {
            node: NodeId(5),
            sign: NodeSign::End,
        };
        assert_eq!(start.to_string(), "-5");
        assert_eq!(end.to_string(), "+5");
    }

    /// §7.13 — the topology violation family routes through the
    /// crate-wide error taxonomy like every other requirements module.
    #[test]
    fn topology_violation_converts_into_cdb_error() {
        let err =
            crate::error::CdbError::from(TopologyViolation::DuplicateNodeId { id: NodeId(9) });
        assert!(matches!(
            err,
            crate::error::CdbError::Topology(TopologyViolation::DuplicateNodeId { id: NodeId(9) })
        ));
    }

    /// Requirement Topo2 /req/core/topology-nodeID (§7.13.4.2; the class
    /// table spells the slug `-nodeid`) — node identifiers are unique: a
    /// duplicate insert is rejected.
    #[test]
    fn req_core_topology_nodeid_duplicate_insert_rejected() {
        let mut graph = TopoGraph::new();
        graph
            .insert_node(TopoNode {
                id: NodeId(1),
                position: None,
            })
            .unwrap();
        assert_eq!(
            graph.insert_node(TopoNode {
                id: NodeId(1),
                position: None
            }),
            Err(TopologyViolation::DuplicateNodeId { id: NodeId(1) })
        );
        graph
            .insert_node(TopoNode {
                id: NodeId(2),
                position: None,
            })
            .unwrap();
        assert_eq!(graph.node_count(), 2);
    }

    /// Requirement Topo3 /req/core/topology-edgeID (§7.13.4.3; table
    /// slug `-edgeid`; the prose's "each node" means "each edge") — edge
    /// identifiers are unique.
    #[test]
    fn req_core_topology_edgeid_duplicate_insert_rejected() {
        let mut graph = TopoGraph::new();
        graph
            .insert_node(TopoNode {
                id: NodeId(1),
                position: None,
            })
            .unwrap();
        graph
            .insert_node(TopoNode {
                id: NodeId(2),
                position: None,
            })
            .unwrap();
        let edge = TopoEdge {
            id: EdgeId(7),
            start: NodeId(1),
            end: NodeId(2),
            geometry: None,
        };
        graph.insert_edge(edge.clone()).unwrap();
        assert_eq!(
            graph.insert_edge(edge),
            Err(TopologyViolation::DuplicateEdgeId { id: EdgeId(7) })
        );
    }

    /// Requirement Topo4 /req/core/topology-node-dir (§7.13.4.4) — after
    /// linking, a shared node's directed view marks the edge leaving it
    /// "−" and the edge entering it "+"; a loop edge contributes both
    /// signs to its single node.
    #[test]
    fn req_core_topology_node_dir_signs_after_linking() {
        let mut graph = TopoGraph::new();
        for id in [1, 2, 3, 5] {
            graph
                .insert_node(TopoNode {
                    id: NodeId(id),
                    position: None,
                })
                .unwrap();
        }
        graph
            .insert_edge(TopoEdge {
                id: EdgeId(10),
                start: NodeId(1),
                end: NodeId(2),
                geometry: None,
            })
            .unwrap();
        graph
            .insert_edge(TopoEdge {
                id: EdgeId(11),
                start: NodeId(2),
                end: NodeId(3),
                geometry: None,
            })
            .unwrap();
        assert_eq!(
            graph.directed_edges(NodeId(2)),
            [
                SignedEdge::Entering(EdgeId(10)),
                SignedEdge::Leaving(EdgeId(11))
            ]
        );
        assert_eq!(
            graph.directed_edges(NodeId(1)),
            [SignedEdge::Leaving(EdgeId(10))]
        );
        graph
            .insert_edge(TopoEdge {
                id: EdgeId(12),
                start: NodeId(5),
                end: NodeId(5),
                geometry: None,
            })
            .unwrap();
        assert_eq!(
            graph.directed_edges(NodeId(5)),
            [
                SignedEdge::Leaving(EdgeId(12)),
                SignedEdge::Entering(EdgeId(12))
            ]
        );
    }

    /// Requirement Topo4 (§7.13.4.4) — the requirement's lead-in exempts
    /// isolated nodes: a node with no incident edges has an empty
    /// directed view (an absent node reads the same, documented).
    #[test]
    fn req_core_topology_node_dir_isolated_node_has_empty_view() {
        let mut graph = TopoGraph::new();
        graph
            .insert_node(TopoNode {
                id: NodeId(1),
                position: Some(geo_types::Point::new(7.5, 51.0)),
            })
            .unwrap();
        assert!(graph.directed_edges(NodeId(1)).is_empty());
        assert!(graph.directed_edges(NodeId(99)).is_empty());
    }

    /// Requirement Topo5 /req/core/topology-edge-dir (§7.13.4.5) — a
    /// directed edge SHALL include existing start and end node IDs; an
    /// unknown endpoint is rejected at insert without leaking adjacency.
    #[test]
    fn req_core_topology_edge_dir_endpoints_must_exist() {
        let mut graph = TopoGraph::new();
        graph
            .insert_node(TopoNode {
                id: NodeId(1),
                position: None,
            })
            .unwrap();
        assert_eq!(
            graph.insert_edge(TopoEdge {
                id: EdgeId(1),
                start: NodeId(1),
                end: NodeId(2),
                geometry: None,
            }),
            Err(TopologyViolation::UnknownNodeId { id: NodeId(2) })
        );
        assert_eq!(
            graph.insert_edge(TopoEdge {
                id: EdgeId(1),
                start: NodeId(3),
                end: NodeId(1),
                geometry: None,
            }),
            Err(TopologyViolation::UnknownNodeId { id: NodeId(3) })
        );
        assert_eq!(graph.edge_count(), 0);
        assert!(
            graph.directed_edges(NodeId(1)).is_empty(),
            "failed inserts must not leak adjacency"
        );
    }

    /// §7.13.1 — "It is possible for two nodes to be connected with no
    /// geometry", and a loop is a legal 1-D primitive; Topo5 holds by
    /// construction (start/end are mandatory fields).
    #[test]
    fn req_core_topology_edge_dir_loop_and_geometryless_edges_legal() {
        let mut graph = TopoGraph::new();
        graph
            .insert_node(TopoNode {
                id: NodeId(1),
                position: None,
            })
            .unwrap();
        assert!(
            graph
                .insert_edge(TopoEdge {
                    id: EdgeId(1),
                    start: NodeId(1),
                    end: NodeId(1),
                    geometry: None,
                })
                .is_ok()
        );
        let stored = graph.edge(EdgeId(1)).unwrap();
        assert_eq!((stored.start, stored.end), (NodeId(1), NodeId(1)));
    }

    /// Triangle fixture: nodes 1,2,3 and Forward edges 10:1→2, 11:2→3,
    /// 12:3→1 — a chained, closed exterior boundary.
    fn triangle_graph() -> TopoGraph {
        let mut graph = TopoGraph::new();
        for id in [1, 2, 3] {
            graph
                .insert_node(TopoNode {
                    id: NodeId(id),
                    position: None,
                })
                .unwrap();
        }
        for (id, start, end) in [(10, 1, 2), (11, 2, 3), (12, 3, 1)] {
            graph
                .insert_edge(TopoEdge {
                    id: EdgeId(id),
                    start: NodeId(start),
                    end: NodeId(end),
                    geometry: None,
                })
                .unwrap();
        }
        graph
    }

    fn forward(edge: u64) -> DirectedEdge {
        DirectedEdge {
            edge: EdgeId(edge),
            orientation: Orientation::Forward,
        }
    }

    /// Face Topology Requirement 2 /req/core/topology-faceID (§7.13.5.3;
    /// table slug `-faceid`) — face identifiers are unique.
    #[test]
    fn req_core_topology_faceid_duplicate_insert_rejected() {
        let mut graph = triangle_graph();
        let boundary = vec![forward(10), forward(11), forward(12)];
        graph
            .insert_face(TopoFace {
                id: FaceId(1),
                boundary: boundary.clone(),
            })
            .unwrap();
        assert_eq!(
            graph.insert_face(TopoFace {
                id: FaceId(1),
                boundary
            }),
            Err(TopologyViolation::DuplicateFaceId { id: FaceId(1) })
        );
        assert_eq!(graph.face_count(), 1);
    }

    /// Face Topology Requirement 3 /req/core/topology-face-structure
    /// (§7.13.5.4; the box carries a `/rec/` prefix but is labeled a
    /// Requirement with SHALL text) and Rec Topology 1
    /// /rec/core/topology-face (§7.13.4.6, identical text) — a face is a
    /// list of directed edges whose ISO directed-node list derives per
    /// step: Forward yields (−start, +end), Reverse yields (+end,
    /// −start).
    #[test]
    fn req_core_topology_face_structure_ring_chains_and_closes() {
        let mut graph = TopoGraph::new();
        for id in [1, 2, 3] {
            graph
                .insert_node(TopoNode {
                    id: NodeId(id),
                    position: None,
                })
                .unwrap();
        }
        // 10: 1→2 used Forward; 11: 3→2 used Reverse (traverses 2→3);
        // 12: 3→1 used Forward.
        for (id, start, end) in [(10, 1, 2), (11, 3, 2), (12, 3, 1)] {
            graph
                .insert_edge(TopoEdge {
                    id: EdgeId(id),
                    start: NodeId(start),
                    end: NodeId(end),
                    geometry: None,
                })
                .unwrap();
        }
        let face = TopoFace {
            id: FaceId(1),
            boundary: vec![
                forward(10),
                DirectedEdge {
                    edge: EdgeId(11),
                    orientation: Orientation::Reverse,
                },
                forward(12),
            ],
        };
        let directed = face.directed_nodes(&graph).unwrap();
        assert_eq!(
            directed,
            vec![
                DirectedNode {
                    node: NodeId(1),
                    sign: NodeSign::Start
                },
                DirectedNode {
                    node: NodeId(2),
                    sign: NodeSign::End
                },
                DirectedNode {
                    node: NodeId(2),
                    sign: NodeSign::End
                },
                DirectedNode {
                    node: NodeId(3),
                    sign: NodeSign::Start
                },
                DirectedNode {
                    node: NodeId(3),
                    sign: NodeSign::Start
                },
                DirectedNode {
                    node: NodeId(1),
                    sign: NodeSign::End
                },
            ]
        );
        graph.insert_face(face).unwrap();
        assert_eq!(graph.face_count(), 1);
    }

    /// Face Topology Requirement 3 (§7.13.5.4) — "exterior boundary (aka
    /// polygon)" entails a chained, closed, non-empty ring (meaning-keyed
    /// module doc note): violations are rejected at insert.
    #[test]
    fn req_core_topology_face_structure_rejects_broken_rings() {
        let mut graph = triangle_graph();
        assert_eq!(
            graph.insert_face(TopoFace {
                id: FaceId(1),
                boundary: Vec::new()
            }),
            Err(TopologyViolation::FaceBoundaryEmpty { face: FaceId(1) })
        );
        // Steps out of order: 1→2 then 3→1 does not chain at step 0.
        assert_eq!(
            graph.insert_face(TopoFace {
                id: FaceId(1),
                boundary: vec![forward(10), forward(12), forward(11)],
            }),
            Err(TopologyViolation::FaceBoundaryNotChained {
                face: FaceId(1),
                step: 0
            })
        );
        // A chained open path: 1→2→3 does not close back to 1.
        assert_eq!(
            graph.insert_face(TopoFace {
                id: FaceId(1),
                boundary: vec![forward(10), forward(11)],
            }),
            Err(TopologyViolation::FaceBoundaryNotClosed { face: FaceId(1) })
        );
        assert_eq!(graph.face_count(), 0);
    }

    /// Face Topology Requirement 3 (§7.13.5.4) — a boundary step
    /// referencing an edge absent from the graph is rejected, both at
    /// insert and in the derived directed-node list.
    #[test]
    fn req_core_topology_face_structure_rejects_unknown_edge() {
        let mut graph = triangle_graph();
        assert_eq!(
            graph.insert_face(TopoFace {
                id: FaceId(1),
                boundary: vec![forward(10), forward(11), forward(99)],
            }),
            Err(TopologyViolation::UnknownEdgeId { id: EdgeId(99) })
        );
        let face = TopoFace {
            id: FaceId(2),
            boundary: vec![forward(99)],
        };
        assert!(matches!(
            face.directed_nodes(&graph),
            Err(TopologyViolation::UnknownEdgeId { id: EdgeId(99) })
        ));
    }

    /// Face Topology Requirement 4 /req/core/topology-winding (class
    /// table; the box says /rec/core/topology-face-winding — draft
    /// quirk), §7.13.5.5 — winding order is clockwise or
    /// counterclockwise with exact wire spellings; unknown values are
    /// violations.
    #[test]
    fn req_core_topology_face_winding_wire_spellings() {
        let expected = [
            (WindingOrder::Clockwise, "clockwise"),
            (WindingOrder::Counterclockwise, "counterclockwise"),
        ];
        assert_eq!(WindingOrder::ALL.len(), expected.len());
        for (variant, wire) in expected {
            assert_eq!(variant.as_str(), wire);
            assert_eq!(variant.to_string(), wire);
            assert_eq!(WindingOrder::parse(wire), Ok(variant));
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, format!("{wire:?}"));
            assert_eq!(
                serde_json::from_str::<WindingOrder>(&json).unwrap(),
                variant
            );
        }
        assert_eq!(
            WindingOrder::parse("widdershins"),
            Err(TopologyViolation::UnknownWindingOrder {
                value: "widdershins".to_owned(),
            })
        );
    }

    /// Face Topology Requirement 4 (§7.13.5.5) — winding order SHALL be
    /// documented in the metadata for the topologically structured
    /// dataset: a graph with faces requires `windingOrder` on its
    /// resource record; declared winding or a face-less graph passes;
    /// the record's own metadata validation is delegated.
    #[test]
    fn req_core_topology_face_winding_required_when_faces_exist() {
        use crate::metadata::ResourceMetadata;

        let mut graph = triangle_graph();
        graph
            .insert_face(TopoFace {
                id: FaceId(1),
                boundary: vec![forward(10), forward(11), forward(12)],
            })
            .unwrap();
        let mut record =
            ResourceMetadata::new("Roads", "Road Topology", "Topologically structured roads");
        assert_eq!(
            validate_topology_dataset(&graph, &record),
            Err(TopologyViolation::WindingOrderUndeclared)
        );
        record.winding_order = Some(WindingOrder::Counterclockwise);
        assert_eq!(validate_topology_dataset(&graph, &record), Ok(()));

        let faceless = triangle_graph();
        let plain = ResourceMetadata::new("Rails", "Rail Topology", "Edge-node graph only");
        assert_eq!(validate_topology_dataset(&faceless, &plain), Ok(()));

        let mut invalid = ResourceMetadata::new("", "T", "D");
        invalid.winding_order = Some(WindingOrder::Clockwise);
        assert!(matches!(
            validate_topology_dataset(&graph, &invalid),
            Err(TopologyViolation::Metadata(_))
        ));
    }

    /// Requirement Topo6 /req/core/topology-clip (§7.13.4.7; the class
    /// table's slug is `-edge-clip`) — an edge crossing a CDB1 tile
    /// boundary is clipped and the minted node is shared: one NodeId, on
    /// the boundary bit-exactly, referenced by the inside and outside
    /// parts; clipping the outside part against the adjacent tile mints
    /// nothing new because adjacent extents agree bitwise (dyadic
    /// boundaries).
    #[test]
    fn req_core_topology_clip_two_tile_edge_shares_one_clip_node() {
        use crate::tiling::{Cdb1GlobalGrid, Cdb1Lod};

        let lod = Cdb1Lod::new(0).unwrap();
        let tile_a = Cdb1GlobalGrid::tile_extent(Cdb1GlobalGrid::address(lod, 89, 180).unwrap());
        let tile_b = Cdb1GlobalGrid::tile_extent(Cdb1GlobalGrid::address(lod, 89, 181).unwrap());
        assert_eq!(
            (tile_a.west, tile_a.south, tile_a.east, tile_a.north),
            (0.0, 0.0, 1.0, 1.0)
        );
        assert_eq!(
            tile_a.east, tile_b.west,
            "adjacent CDB1 extents agree bitwise"
        );

        let mut graph = TopoGraph::new();
        graph
            .insert_node(TopoNode {
                id: NodeId(1),
                position: Some(geo_types::Point::new(0.25, 0.5)),
            })
            .unwrap();
        graph
            .insert_node(TopoNode {
                id: NodeId(2),
                position: Some(geo_types::Point::new(1.75, 0.5)),
            })
            .unwrap();
        graph
            .insert_edge(TopoEdge {
                id: EdgeId(1),
                start: NodeId(1),
                end: NodeId(2),
                geometry: None,
            })
            .unwrap();

        let outcome = graph.clip_edge_to_tile(EdgeId(1), tile_a).unwrap();
        assert_eq!(outcome.inside.len(), 1);
        assert_eq!(outcome.outside.len(), 1);
        assert_eq!(outcome.clip_nodes.len(), 1);
        assert!(
            graph.edge(EdgeId(1)).is_none(),
            "the crossing edge is replaced by its parts"
        );

        let clip = outcome.clip_nodes[0];
        let position = graph.node(clip).unwrap().position.unwrap();
        assert_eq!(
            position.x(),
            tile_a.east,
            "boundary coordinate pinned bit-exactly"
        );
        assert_eq!(position.x(), tile_b.west);
        assert_eq!(position.y(), 0.5);

        // Requirement Topo5: every part is a directed edge chaining the
        // original nodes through the shared clip node ("artificial
        // (virtual) nodes generated by processes that clip edges").
        let inside = graph.edge(outcome.inside[0]).unwrap();
        let outside = graph.edge(outcome.outside[0]).unwrap();
        assert_eq!((inside.start, inside.end), (NodeId(1), clip));
        assert_eq!((outside.start, outside.end), (clip, NodeId(2)));
        // Requirement Topo4: the shared node's directed view sees both.
        assert_eq!(
            graph.directed_edges(clip),
            [
                SignedEdge::Entering(outcome.inside[0]),
                SignedEdge::Leaving(outcome.outside[0])
            ]
        );

        // The outside part lies in tile B; its west boundary passes
        // through the shared node, so a second clip mints nothing.
        let second = graph.clip_edge_to_tile(outcome.outside[0], tile_b).unwrap();
        assert_eq!(second.clip_nodes, Vec::<NodeId>::new());
        assert_eq!(second.inside, vec![outcome.outside[0]]);
        assert!(second.outside.is_empty());
        assert_eq!(graph.node(clip).unwrap().position.unwrap().x(), tile_b.west);
    }

    /// Requirement Topo6 (§7.13.4.7) — only a crossing edge is clipped:
    /// a fully-inside or fully-outside edge is untouched and reported
    /// whole in its bucket.
    #[test]
    fn req_core_topology_clip_non_crossing_edge_untouched() {
        let tile = Bbox {
            west: 0.0,
            south: 0.0,
            east: 1.0,
            north: 1.0,
        };
        let mut graph = TopoGraph::new();
        for (id, x, y) in [(1, 0.2, 0.2), (2, 0.8, 0.9), (3, 2.0, 2.0), (4, 3.0, 2.5)] {
            graph
                .insert_node(TopoNode {
                    id: NodeId(id),
                    position: Some(geo_types::Point::new(x, y)),
                })
                .unwrap();
        }
        graph
            .insert_edge(TopoEdge {
                id: EdgeId(1),
                start: NodeId(1),
                end: NodeId(2),
                geometry: None,
            })
            .unwrap();
        graph
            .insert_edge(TopoEdge {
                id: EdgeId(2),
                start: NodeId(3),
                end: NodeId(4),
                geometry: None,
            })
            .unwrap();

        let inside = graph.clip_edge_to_tile(EdgeId(1), tile).unwrap();
        assert_eq!(
            inside,
            EdgeClipOutcome {
                inside: vec![EdgeId(1)],
                outside: Vec::new(),
                clip_nodes: Vec::new(),
            }
        );
        let outside = graph.clip_edge_to_tile(EdgeId(2), tile).unwrap();
        assert_eq!(
            outside,
            EdgeClipOutcome {
                inside: Vec::new(),
                outside: vec![EdgeId(2)],
                clip_nodes: Vec::new(),
            }
        );
        assert_eq!(
            graph.edge_count(),
            2,
            "no-crossing clips leave the graph untouched"
        );
    }

    /// Requirement Topo6 (§7.13.4.7) preconditions — no derivable
    /// polyline, detached geometry, an unknown edge, and a degenerate
    /// extent are rejected before any mutation.
    #[test]
    fn req_core_topology_clip_preconditions_rejected() {
        let tile = Bbox {
            west: 0.0,
            south: 0.0,
            east: 1.0,
            north: 1.0,
        };
        let mut graph = TopoGraph::new();
        graph
            .insert_node(TopoNode {
                id: NodeId(1),
                position: None,
            })
            .unwrap();
        graph
            .insert_node(TopoNode {
                id: NodeId(2),
                position: Some(geo_types::Point::new(0.5, 0.5)),
            })
            .unwrap();
        graph
            .insert_edge(TopoEdge {
                id: EdgeId(1),
                start: NodeId(1),
                end: NodeId(2),
                geometry: None,
            })
            .unwrap();
        assert_eq!(
            graph.clip_edge_to_tile(EdgeId(1), tile),
            Err(TopologyViolation::EdgeHasNoGeometry { id: EdgeId(1) })
        );

        graph
            .insert_node(TopoNode {
                id: NodeId(3),
                position: Some(geo_types::Point::new(0.3, 0.5)),
            })
            .unwrap();
        let detached = geo_types::LineString::from(vec![(0.4, 0.5), (1.5, 0.5)]);
        graph
            .insert_edge(TopoEdge {
                id: EdgeId(2),
                start: NodeId(3),
                end: NodeId(2),
                geometry: Some(detached),
            })
            .unwrap();
        assert_eq!(
            graph.clip_edge_to_tile(EdgeId(2), tile),
            Err(TopologyViolation::EdgeGeometryEndpointMismatch { id: EdgeId(2) })
        );

        assert_eq!(
            graph.clip_edge_to_tile(EdgeId(99), tile),
            Err(TopologyViolation::UnknownEdgeId { id: EdgeId(99) })
        );

        let degenerate = Bbox {
            west: 10.0,
            south: 0.0,
            east: -10.0,
            north: 1.0,
        };
        assert_eq!(
            graph.clip_edge_to_tile(EdgeId(2), degenerate),
            Err(TopologyViolation::InvalidClipExtent {
                west: 10.0,
                south: 0.0,
                east: -10.0,
                north: 1.0,
            })
        );
        assert_eq!(
            graph.edge_count(),
            2,
            "failed clips leave the graph untouched"
        );
    }

    /// Requirement Topo6 (§7.13.4.7) is titled "Clipping edges" — an
    /// edge bounding a face is refused (face/polygon clipping is a
    /// profile concern; silently invalidating a face is unacceptable).
    #[test]
    fn req_core_topology_clip_edge_bounding_a_face_rejected() {
        let mut graph = triangle_graph();
        graph
            .insert_face(TopoFace {
                id: FaceId(4),
                boundary: vec![forward(10), forward(11), forward(12)],
            })
            .unwrap();
        assert_eq!(
            graph.clip_edge_to_tile(
                EdgeId(10),
                Bbox {
                    west: 0.0,
                    south: 0.0,
                    east: 1.0,
                    north: 1.0
                }
            ),
            Err(TopologyViolation::EdgeInFace {
                edge: EdgeId(10),
                face: FaceId(4)
            })
        );
    }

    /// §7.13.4.7 NOTE ("an edge that ends right at the corner of a
    /// tile") — closed containment: an endpoint exactly on the tile
    /// corner or boundary is a touch, not a crossing; nothing is minted
    /// on either side of the boundary.
    #[test]
    fn req_core_topology_clip_endpoint_corner_touch_is_not_a_crossing() {
        let tile = Bbox {
            west: 0.0,
            south: 0.0,
            east: 1.0,
            north: 1.0,
        };
        let mut graph = TopoGraph::new();
        // Inside edge ending exactly at the (1, 1) corner.
        graph
            .insert_node(TopoNode {
                id: NodeId(1),
                position: Some(geo_types::Point::new(0.5, 0.5)),
            })
            .unwrap();
        graph
            .insert_node(TopoNode {
                id: NodeId(2),
                position: Some(geo_types::Point::new(1.0, 1.0)),
            })
            .unwrap();
        graph
            .insert_edge(TopoEdge {
                id: EdgeId(1),
                start: NodeId(1),
                end: NodeId(2),
                geometry: None,
            })
            .unwrap();
        let outcome = graph.clip_edge_to_tile(EdgeId(1), tile).unwrap();
        assert_eq!(
            outcome,
            EdgeClipOutcome {
                inside: vec![EdgeId(1)],
                outside: Vec::new(),
                clip_nodes: Vec::new(),
            }
        );
        // Edge starting exactly on the east boundary and leaving: the
        // boundary point is the original node, so nothing is minted and
        // the edge is whole in the outside bucket.
        graph
            .insert_node(TopoNode {
                id: NodeId(3),
                position: Some(geo_types::Point::new(1.0, 0.5)),
            })
            .unwrap();
        graph
            .insert_node(TopoNode {
                id: NodeId(4),
                position: Some(geo_types::Point::new(1.5, 0.5)),
            })
            .unwrap();
        graph
            .insert_edge(TopoEdge {
                id: EdgeId(2),
                start: NodeId(3),
                end: NodeId(4),
                geometry: None,
            })
            .unwrap();
        let outcome = graph.clip_edge_to_tile(EdgeId(2), tile).unwrap();
        assert_eq!(
            outcome,
            EdgeClipOutcome {
                inside: Vec::new(),
                outside: vec![EdgeId(2)],
                clip_nodes: Vec::new(),
            }
        );
    }

    /// §7.13.4.7 NOTE — a segment passing exactly through the tile
    /// corner from outside to outside grazes the closed boundary at one
    /// point: a touch, not a crossing.
    #[test]
    fn req_core_topology_clip_outside_graze_through_corner_mints_nothing() {
        let tile = Bbox {
            west: 0.0,
            south: 0.0,
            east: 1.0,
            north: 1.0,
        };
        let mut graph = TopoGraph::new();
        graph
            .insert_node(TopoNode {
                id: NodeId(1),
                position: Some(geo_types::Point::new(1.5, 0.5)),
            })
            .unwrap();
        graph
            .insert_node(TopoNode {
                id: NodeId(2),
                position: Some(geo_types::Point::new(0.5, 1.5)),
            })
            .unwrap();
        graph
            .insert_edge(TopoEdge {
                id: EdgeId(1),
                start: NodeId(1),
                end: NodeId(2),
                geometry: None,
            })
            .unwrap();
        let outcome = graph.clip_edge_to_tile(EdgeId(1), tile).unwrap();
        assert_eq!(
            outcome,
            EdgeClipOutcome {
                inside: Vec::new(),
                outside: vec![EdgeId(1)],
                clip_nodes: Vec::new(),
            }
        );
    }

    /// §7.13.4.7 — a polyline crossing the boundary exactly through one
    /// of its interior vertices mints the clip node at that vertex's
    /// coordinates.
    #[test]
    fn req_core_topology_clip_vertex_on_boundary_crossing_mints_at_vertex() {
        let tile = Bbox {
            west: 0.0,
            south: 0.0,
            east: 1.0,
            north: 1.0,
        };
        let mut graph = TopoGraph::new();
        graph
            .insert_node(TopoNode {
                id: NodeId(1),
                position: Some(geo_types::Point::new(0.5, 0.5)),
            })
            .unwrap();
        graph
            .insert_node(TopoNode {
                id: NodeId(2),
                position: Some(geo_types::Point::new(1.5, 0.5)),
            })
            .unwrap();
        let polyline = geo_types::LineString::from(vec![(0.5, 0.5), (1.0, 0.5), (1.5, 0.5)]);
        graph
            .insert_edge(TopoEdge {
                id: EdgeId(1),
                start: NodeId(1),
                end: NodeId(2),
                geometry: Some(polyline),
            })
            .unwrap();
        let outcome = graph.clip_edge_to_tile(EdgeId(1), tile).unwrap();
        assert_eq!(outcome.clip_nodes.len(), 1);
        let position = graph.node(outcome.clip_nodes[0]).unwrap().position.unwrap();
        assert_eq!((position.x(), position.y()), (1.0, 0.5));
    }

    /// §7.13.4.7 (closed containment, module-doc'd decision) — a run
    /// collinear along the boundary lies inside the closed tile: no
    /// crossing, no nodes.
    #[test]
    fn req_core_topology_clip_collinear_boundary_run_stays_inside() {
        let tile = Bbox {
            west: 0.0,
            south: 0.0,
            east: 1.0,
            north: 1.0,
        };
        let mut graph = TopoGraph::new();
        graph
            .insert_node(TopoNode {
                id: NodeId(1),
                position: Some(geo_types::Point::new(1.0, 0.25)),
            })
            .unwrap();
        graph
            .insert_node(TopoNode {
                id: NodeId(2),
                position: Some(geo_types::Point::new(1.0, 0.75)),
            })
            .unwrap();
        graph
            .insert_edge(TopoEdge {
                id: EdgeId(1),
                start: NodeId(1),
                end: NodeId(2),
                geometry: None,
            })
            .unwrap();
        let outcome = graph.clip_edge_to_tile(EdgeId(1), tile).unwrap();
        assert_eq!(
            outcome,
            EdgeClipOutcome {
                inside: vec![EdgeId(1)],
                outside: Vec::new(),
                clip_nodes: Vec::new(),
            }
        );
    }

    /// §7.13.4.7 — an edge crossing the same boundary repeatedly is
    /// split at every crossing: parts alternate inside/outside with one
    /// minted node per crossing, every crossing pinned to the boundary.
    #[test]
    fn req_core_topology_clip_zigzag_multi_crossing_alternates_parts() {
        let tile = Bbox {
            west: 0.0,
            south: 0.0,
            east: 1.0,
            north: 1.0,
        };
        let mut graph = TopoGraph::new();
        graph
            .insert_node(TopoNode {
                id: NodeId(1),
                position: Some(geo_types::Point::new(0.5, 0.5)),
            })
            .unwrap();
        graph
            .insert_node(TopoNode {
                id: NodeId(2),
                position: Some(geo_types::Point::new(0.5, 0.25)),
            })
            .unwrap();
        let polyline = geo_types::LineString::from(vec![(0.5, 0.5), (1.5, 0.5), (0.5, 0.25)]);
        graph
            .insert_edge(TopoEdge {
                id: EdgeId(1),
                start: NodeId(1),
                end: NodeId(2),
                geometry: Some(polyline),
            })
            .unwrap();
        let outcome = graph.clip_edge_to_tile(EdgeId(1), tile).unwrap();
        assert_eq!(outcome.clip_nodes.len(), 2);
        assert_eq!(outcome.inside.len(), 2);
        assert_eq!(outcome.outside.len(), 1);
        for id in &outcome.clip_nodes {
            let position = graph.node(*id).unwrap().position.unwrap();
            assert_eq!(
                position.x(),
                1.0,
                "every crossing pins the east boundary exactly"
            );
        }
        // Second crossing's y interpolates on the return segment:
        // t = (1.0 − 1.5)/(0.5 − 1.5) = 0.5 → y = 0.5 + 0.5·(0.25 − 0.5)
        // = 0.375, all dyadic and exact.
        let second = graph.node(outcome.clip_nodes[1]).unwrap().position.unwrap();
        assert_eq!(second.y(), 0.375);
    }

    /// Requirement Topo6 (§7.13.4.7) — the clip is grid-agnostic: a
    /// GNOSISGlobalGrid level-0 extent (§7.12, the 2×4 grid of 90°
    /// tiles) drives the same operation through the same `Bbox` surface.
    #[test]
    fn req_core_topology_clip_gnosis_extent_grid_agnostic() {
        use crate::tiling::{GnosisGlobalGrid, GnosisLevel};

        let level = GnosisLevel::new(0).unwrap();
        let tile = GnosisGlobalGrid::tile_extent(GnosisGlobalGrid::address(level, 0, 1).unwrap());
        assert_eq!(
            (tile.west, tile.south, tile.east, tile.north),
            (-90.0, 0.0, 0.0, 90.0)
        );

        let mut graph = TopoGraph::new();
        graph
            .insert_node(TopoNode {
                id: NodeId(1),
                position: Some(geo_types::Point::new(-45.0, 45.0)),
            })
            .unwrap();
        graph
            .insert_node(TopoNode {
                id: NodeId(2),
                position: Some(geo_types::Point::new(45.0, 45.0)),
            })
            .unwrap();
        graph
            .insert_edge(TopoEdge {
                id: EdgeId(1),
                start: NodeId(1),
                end: NodeId(2),
                geometry: None,
            })
            .unwrap();
        let outcome = graph.clip_edge_to_tile(EdgeId(1), tile).unwrap();
        assert_eq!(outcome.clip_nodes.len(), 1);
        let position = graph.node(outcome.clip_nodes[0]).unwrap().position.unwrap();
        assert_eq!((position.x(), position.y()), (0.0, 45.0));
    }

    /// §7.13.4.7 NOTE (precision) — geo's robust segment intersection is
    /// the independent oracle: the minted node lies on both the segment
    /// and the boundary line, and the pinned x is exact where the
    /// generic formula may not be.
    #[test]
    fn req_core_topology_clip_oracle_geo_line_intersection_agrees() {
        use geo::LineIntersection;
        use geo::line_intersection::line_intersection;

        let tile = Bbox {
            west: 0.0,
            south: 0.0,
            east: 1.0,
            north: 1.0,
        };
        let mut graph = TopoGraph::new();
        graph
            .insert_node(TopoNode {
                id: NodeId(1),
                position: Some(geo_types::Point::new(0.25, 0.1)),
            })
            .unwrap();
        graph
            .insert_node(TopoNode {
                id: NodeId(2),
                position: Some(geo_types::Point::new(1.6, 0.85)),
            })
            .unwrap();
        graph
            .insert_edge(TopoEdge {
                id: EdgeId(1),
                start: NodeId(1),
                end: NodeId(2),
                geometry: None,
            })
            .unwrap();
        let outcome = graph.clip_edge_to_tile(EdgeId(1), tile).unwrap();
        assert_eq!(outcome.clip_nodes.len(), 1);
        let minted = graph.node(outcome.clip_nodes[0]).unwrap().position.unwrap();
        assert_eq!(minted.x(), 1.0, "the boundary coordinate is pinned exactly");

        let segment = geo_types::Line::new(coord! { x: 0.25, y: 0.1 }, coord! { x: 1.6, y: 0.85 });
        let boundary = geo_types::Line::new(coord! { x: 1.0, y: 0.0 }, coord! { x: 1.0, y: 1.0 });
        match line_intersection(segment, boundary) {
            Some(LineIntersection::SinglePoint { intersection, .. }) => {
                assert!((intersection.x - minted.x()).abs() < 1e-12);
                assert!((intersection.y - minted.y()).abs() < 1e-12);
            }
            other => panic!("oracle disagrees with the clip: {other:?}"),
        }
    }

    /// §7.13.4.7 NOTE — final-review regression: a NON-dyadic polyline
    /// whose endpoint lies exactly on the tile corner, approached from
    /// outside, is a touch. Before the endpoint-exact lerp fix, t = 1.0
    /// interpolation recomputed the vertex ~1 ulp off, minting a phantom
    /// out-of-tile node and a bogus "inside" part.
    #[test]
    fn req_core_topology_clip_boundary_vertex_from_outside_is_a_touch() {
        let tile = Bbox {
            west: 3.25,
            south: 1.75,
            east: 6.75,
            north: 3.75,
        };
        let mut graph = TopoGraph::new();
        graph
            .insert_node(TopoNode {
                id: NodeId(1),
                position: Some(geo_types::Point::new(2.75, -0.8979679231350227)),
            })
            .unwrap();
        graph
            .insert_node(TopoNode {
                id: NodeId(2),
                position: Some(geo_types::Point::new(3.25, 3.75)),
            })
            .unwrap();
        graph
            .insert_edge(TopoEdge {
                id: EdgeId(1),
                start: NodeId(1),
                end: NodeId(2),
                geometry: None,
            })
            .unwrap();
        let outcome = graph.clip_edge_to_tile(EdgeId(1), tile).unwrap();
        assert_eq!(
            outcome,
            EdgeClipOutcome {
                inside: Vec::new(),
                outside: vec![EdgeId(1)],
                clip_nodes: Vec::new(),
            }
        );
        assert_eq!(graph.node_count(), 2, "no phantom nodes minted");
    }

    /// §7.13.4.7 (final-review) — non-finite coordinates are rejected as a
    /// clip precondition instead of minting NaN nodes.
    #[test]
    fn req_core_topology_clip_rejects_non_finite_polyline() {
        let tile = Bbox {
            west: 0.0,
            south: 0.0,
            east: 1.0,
            north: 1.0,
        };
        let mut graph = TopoGraph::new();
        graph
            .insert_node(TopoNode {
                id: NodeId(1),
                position: Some(geo_types::Point::new(f64::NAN, 0.5)),
            })
            .unwrap();
        graph
            .insert_node(TopoNode {
                id: NodeId(2),
                position: Some(geo_types::Point::new(1.5, 0.5)),
            })
            .unwrap();
        graph
            .insert_edge(TopoEdge {
                id: EdgeId(1),
                start: NodeId(1),
                end: NodeId(2),
                geometry: None,
            })
            .unwrap();
        assert_eq!(
            graph.clip_edge_to_tile(EdgeId(1), tile),
            Err(TopologyViolation::EdgeGeometryNotFinite { id: EdgeId(1) })
        );
        let infinite = geo_types::LineString::from(vec![(0.5, 0.5), (f64::INFINITY, 0.5)]);
        graph
            .insert_node(TopoNode {
                id: NodeId(3),
                position: None,
            })
            .unwrap();
        graph
            .insert_node(TopoNode {
                id: NodeId(4),
                position: None,
            })
            .unwrap();
        graph
            .insert_edge(TopoEdge {
                id: EdgeId(2),
                start: NodeId(3),
                end: NodeId(4),
                geometry: Some(infinite),
            })
            .unwrap();
        assert_eq!(
            graph.clip_edge_to_tile(EdgeId(2), tile),
            Err(TopologyViolation::EdgeGeometryNotFinite { id: EdgeId(2) })
        );
        assert_eq!(
            graph.edge_count(),
            2,
            "failed clips leave the graph untouched"
        );
    }

    /// Final-review hardening: a graph holding ids at the top of the u64
    /// range refuses to clip BEFORE any mutation, keeping the
    /// errors-all-pre-mutation contract true even at counter saturation.
    #[test]
    fn req_core_topology_clip_id_headroom_guard_is_pre_mutation() {
        let tile = Bbox {
            west: 0.0,
            south: 0.0,
            east: 1.0,
            north: 1.0,
        };
        let mut graph = TopoGraph::new();
        graph
            .insert_node(TopoNode {
                id: NodeId(u64::MAX),
                position: Some(geo_types::Point::new(0.5, 0.5)),
            })
            .unwrap();
        graph
            .insert_node(TopoNode {
                id: NodeId(1),
                position: Some(geo_types::Point::new(1.5, 0.5)),
            })
            .unwrap();
        graph
            .insert_edge(TopoEdge {
                id: EdgeId(1),
                start: NodeId(u64::MAX),
                end: NodeId(1),
                geometry: None,
            })
            .unwrap();
        assert_eq!(
            graph.clip_edge_to_tile(EdgeId(1), tile),
            Err(TopologyViolation::DuplicateNodeId {
                id: NodeId(u64::MAX)
            })
        );
        assert!(
            graph.edge(EdgeId(1)).is_some(),
            "guard fires before any mutation"
        );
        assert_eq!(graph.edge_count(), 1);
    }

    /// §7.13.4.7 (final-review) — an outside polyline ENDING exactly on
    /// the boundary is a touch: the terminal single-coordinate run is
    /// dropped and its crossing cancelled (the post-pass `pop` branch).
    #[test]
    fn req_core_topology_clip_endpoint_lands_on_boundary_from_outside() {
        let tile = Bbox {
            west: 0.0,
            south: 0.0,
            east: 1.0,
            north: 1.0,
        };
        let mut graph = TopoGraph::new();
        graph
            .insert_node(TopoNode {
                id: NodeId(1),
                position: Some(geo_types::Point::new(1.5, 0.5)),
            })
            .unwrap();
        graph
            .insert_node(TopoNode {
                id: NodeId(2),
                position: Some(geo_types::Point::new(1.0, 0.5)),
            })
            .unwrap();
        graph
            .insert_edge(TopoEdge {
                id: EdgeId(1),
                start: NodeId(1),
                end: NodeId(2),
                geometry: None,
            })
            .unwrap();
        let outcome = graph.clip_edge_to_tile(EdgeId(1), tile).unwrap();
        assert_eq!(
            outcome,
            EdgeClipOutcome {
                inside: Vec::new(),
                outside: vec![EdgeId(1)],
                clip_nodes: Vec::new(),
            }
        );
    }

    /// Final-review property mini-sweep (seeded, deterministic): 300
    /// random non-dyadic segments against dyadic tiles. Invariants: every
    /// minted node pins at least one coordinate bitwise to a Bbox field;
    /// parts chain start → clips → end (Topo5); re-clipping every part
    /// against the same tile mints nothing (idempotence).
    #[test]
    fn req_core_topology_clip_seeded_property_mini_sweep() {
        let mut state: u64 = 0x5DEECE66D;
        let mut next = move || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 11) as f64 / (1u64 << 53) as f64
        };
        let tile = Bbox {
            west: -0.5,
            south: -0.25,
            east: 1.75,
            north: 1.5,
        };
        for i in 0..300 {
            let mut graph = TopoGraph::new();
            let a = geo_types::Point::new(next() * 6.0 - 3.0, next() * 6.0 - 3.0);
            let b = geo_types::Point::new(next() * 6.0 - 3.0, next() * 6.0 - 3.0);
            graph
                .insert_node(TopoNode {
                    id: NodeId(1),
                    position: Some(a),
                })
                .unwrap();
            graph
                .insert_node(TopoNode {
                    id: NodeId(2),
                    position: Some(b),
                })
                .unwrap();
            graph
                .insert_edge(TopoEdge {
                    id: EdgeId(1),
                    start: NodeId(1),
                    end: NodeId(2),
                    geometry: None,
                })
                .unwrap();
            let outcome = graph.clip_edge_to_tile(EdgeId(1), tile).unwrap();
            for id in &outcome.clip_nodes {
                let p = graph.node(*id).unwrap().position.unwrap();
                assert!(
                    p.x() == tile.west
                        || p.x() == tile.east
                        || p.y() == tile.south
                        || p.y() == tile.north,
                    "iteration {i}: clip node not pinned to a boundary: {p:?}"
                );
            }
            let parts: Vec<EdgeId> = outcome
                .inside
                .iter()
                .chain(outcome.outside.iter())
                .copied()
                .collect();
            for part in &parts {
                let second = graph.clip_edge_to_tile(*part, tile).unwrap();
                assert!(
                    second.clip_nodes.is_empty(),
                    "iteration {i}: re-clip of part {part} minted nodes"
                );
            }
        }
    }
}
