//! Integration: a topologically structured road network spanning two
//! adjacent CDB1GlobalGrid tiles (Requirement Topo6, §7.13.4.7) plus the
//! Face4 winding-order metadata tie-in (§7.13.5.5) — exercised through
//! the crate's public API only.

use rusty_cdb::metadata::ResourceMetadata;
use rusty_cdb::{
    Cdb1GlobalGrid, Cdb1Lod, DirectedEdge, EdgeId, FaceId, NodeId, Orientation, SignedEdge,
    TopoEdge, TopoFace, TopoGraph, TopoNode, TopologyViolation, WindingOrder,
    validate_topology_dataset,
};

/// Topo6 /req/core/topology-clip (§7.13.4.7) over real CDB1 extents —
/// clip a two-tile road at tile A, then re-clip the outside part
/// against neighbour B: one shared junction node, bit-equal boundary
/// coordinates, and Topo4/Topo5 integrity on every part.
#[test]
fn req_core_topology_clip_network_across_two_cdb1_tiles() {
    let lod = Cdb1Lod::new(0).unwrap();
    let tile_a = Cdb1GlobalGrid::tile_extent(Cdb1GlobalGrid::address(lod, 89, 180).unwrap());
    let tile_b = Cdb1GlobalGrid::tile_extent(Cdb1GlobalGrid::address(lod, 89, 181).unwrap());
    assert_eq!(
        tile_a.east, tile_b.west,
        "shared boundary is bitwise-identical"
    );

    let mut graph = TopoGraph::new();
    // Junction J in tile A, road end P in tile B, road end Q in tile A.
    graph
        .insert_node(TopoNode {
            id: NodeId(1),
            position: Some(geo_types::Point::new(0.5, 0.5)),
        })
        .unwrap();
    graph
        .insert_node(TopoNode {
            id: NodeId(2),
            position: Some(geo_types::Point::new(1.5, 0.25)),
        })
        .unwrap();
    graph
        .insert_node(TopoNode {
            id: NodeId(3),
            position: Some(geo_types::Point::new(0.25, 0.9)),
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
    graph
        .insert_edge(TopoEdge {
            id: EdgeId(2),
            start: NodeId(1),
            end: NodeId(3),
            geometry: None,
        })
        .unwrap();

    let outcome = graph.clip_edge_to_tile(EdgeId(1), tile_a).unwrap();
    assert_eq!(
        (
            outcome.inside.len(),
            outcome.outside.len(),
            outcome.clip_nodes.len()
        ),
        (1, 1, 1)
    );
    let junction = outcome.clip_nodes[0];
    let position = graph.node(junction).unwrap().position.unwrap();
    assert_eq!(position.x(), tile_a.east);
    // t = (1.0 − 0.5)/(1.5 − 0.5) = 0.5 → y = 0.5 + 0.5·(0.25 − 0.5) =
    // 0.375, all dyadic and exact.
    assert_eq!(position.y(), 0.375);

    // The whole-in-A road is untouched by the A clip.
    let in_a = graph.clip_edge_to_tile(EdgeId(2), tile_a).unwrap();
    assert!(in_a.clip_nodes.is_empty());
    assert_eq!(in_a.inside, vec![EdgeId(2)]);

    // Clip the outside part against tile B: the shared junction survives
    // with the same identifier and no new node is minted (Topo6's "SHALL
    // share the same node identifier in both tiles").
    let part_b = outcome.outside[0];
    let second = graph.clip_edge_to_tile(part_b, tile_b).unwrap();
    assert!(second.clip_nodes.is_empty());
    assert_eq!(second.inside, vec![part_b]);
    let stored = graph.edge(part_b).unwrap();
    assert_eq!(stored.start, junction);
    assert_eq!(
        graph.node(junction).unwrap().position.unwrap().x(),
        tile_b.west
    );

    // Topo4 at the junction: the A-side part enters, the B-side leaves.
    assert_eq!(
        graph.directed_edges(junction),
        [
            SignedEdge::Entering(outcome.inside[0]),
            SignedEdge::Leaving(part_b)
        ]
    );
}

/// Face2–4 (§7.13.5) — faces over the network dataset: ring
/// construction, the windingOrder ride-along on ResourceMetadata (JSON
/// round-trip), and dataset validation failing exactly when winding is
/// undeclared.
#[test]
fn req_core_topology_face_winding_dataset_roundtrip() {
    let mut graph = TopoGraph::new();
    for (id, x, y) in [(1, 0.1, 0.1), (2, 0.3, 0.1), (3, 0.2, 0.3)] {
        graph
            .insert_node(TopoNode {
                id: NodeId(id),
                position: Some(geo_types::Point::new(x, y)),
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
    let boundary: Vec<DirectedEdge> = [10, 11, 12]
        .into_iter()
        .map(|edge| DirectedEdge {
            edge: EdgeId(edge),
            orientation: Orientation::Forward,
        })
        .collect();
    graph
        .insert_face(TopoFace {
            id: FaceId(1),
            boundary,
        })
        .unwrap();

    let mut record =
        ResourceMetadata::new("RoadFaces", "Road Polygons", "Faces over the road network");
    assert_eq!(
        validate_topology_dataset(&graph, &record),
        Err(TopologyViolation::WindingOrderUndeclared)
    );
    record.winding_order = Some(WindingOrder::Counterclockwise);
    validate_topology_dataset(&graph, &record).unwrap();

    let json = record.to_json_string().unwrap();
    assert!(json.contains("\"windingOrder\"") && json.contains("counterclockwise"));
    let back = ResourceMetadata::from_json_str(&json).unwrap();
    assert_eq!(back.winding_order, Some(WindingOrder::Counterclockwise));
    validate_topology_dataset(&graph, &back).unwrap();
}
