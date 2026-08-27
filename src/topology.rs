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

use std::collections::BTreeMap;
use std::fmt;

use thiserror::Error;

use crate::metadata::{MetadataViolation, ResourceMetadata};

/// Unique node identifier (Requirement Topo2,
/// /req/core/topology-nodeID, §7.13.4.2; the class table spells the slug
/// `-nodeid`). The spec leaves ID structure open ("an integer number or
/// a combination of a tile identifier and an integer number"); the core
/// picks plain integers and leaves tile-scoped composition to
/// application profiles. Crosswalk: CDB 1.x SJID/EJID/JID (§7.13.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u64);

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique edge identifier (Requirement Topo3,
/// /req/core/topology-edgeID, §7.13.4.3; table slug `-edgeid`; the
/// section prose's "each node" means "each edge" — draft quirk).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EdgeId(pub u64);

impl fmt::Display for EdgeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique face identifier (Face Topology Requirement 2,
/// /req/core/topology-faceID, §7.13.5.3; table slug `-faceid`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// Traverse the edge start→end.
    Forward,
    /// Traverse the edge end→start.
    Reverse,
}

/// ISO 19107 directed edge: an association between an edge and one of
/// its two orientations (§7.13.1). Face boundaries are lists of these
/// (Face Topology Requirement 3, §7.13.5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// A topologically structured vector dataset: the edge-node(-face) graph
/// of §7.13.1, enforcing the module's SHALLs at insert time — duplicate
/// identifiers are unrepresentable (Topo2/Topo3/Face2), every edge's
/// endpoints must exist (Topo5), and the signed directed-node adjacency
/// (Topo4) is maintained by the graph itself, so a wrong sign cannot be
/// constructed. The only mutations are the `insert_*` methods and the
/// Topo6 clip operation; there is no public delete API.
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
}
