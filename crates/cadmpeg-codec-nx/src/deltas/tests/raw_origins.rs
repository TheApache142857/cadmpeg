// SPDX-License-Identifier: Apache-2.0
use crate::raw_origin::{RawRecordOrigin, RawSource, RawStreamOrdinal};

#[test]
fn primary_records_keep_original_source_identity_and_offsets() {
    let bytes = crate::test_support::test_streams::topology_partition_stream();
    crate::test_support::with_decode_context(|ctx| {
        let source = RawSource::raw(RawStreamOrdinal::new(4), &bytes);
        let graph = crate::topology::Graph::parse(ctx, source.bytes()).unwrap();
        for (kind, xmt) in [
            (crate::framing::node_kind::NodeKind::Point, 11),
            (crate::framing::node_kind::NodeKind::Shell, 3),
        ] {
            let node = graph.get(kind, xmt).unwrap();
            assert_eq!(
                source.origins().record_start(ctx, node.pos()).unwrap(),
                RawRecordOrigin {
                    stream: RawStreamOrdinal::new(4),
                    offset: node.pos()
                }
            );
        }
        assert!(std::ptr::eq(source.bytes().as_ptr(), bytes.as_ptr()));
    });
}

#[test]
fn deltas_owned_field_ranges_are_raw_offsets_and_never_canonical_positions() {
    let bytes = crate::test_support::test_pmarks::complete(
        crate::test_support::owned_deltas::body_stream(36),
    );
    crate::test_support::with_decode_context(|ctx| {
        let owner = crate::records::DeltasRecords::read(ctx, &bytes).unwrap();
        let crate::records::deltas::EventContent::Instance(instance) = owner.events()[2].content()
        else {
            panic!("BODY instance");
        };
        for span in instance.fields() {
            assert!(span.start >= instance.identity_at());
            assert!(span.end <= bytes.len());
            assert_eq!(owner.source().get(span.clone()), bytes.get(span.clone()));
        }
        assert!(std::ptr::eq(owner.source().as_ptr(), bytes.as_ptr()));
    });
}
