// SPDX-License-Identifier: Apache-2.0
//! Shared primary parses and sequential Deltas source ownership.
//! Deltas streams have an empty semantic view; their physical events are retained separately.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::decode::Scan;
use crate::intersection::{self, CurveScan};
use crate::parasolid::{Stream, StreamKind};
use crate::raw_origin::{RawSource, RawStreamOrdinal};
use crate::topology::{BlendSurface, Graph, OffsetSurface, SurfaceCurve, TrimmedCurve};

struct TopologyStream<'a> {
    source: RawSource<'a>,
    deltas: Option<crate::records::DeltasRecords<'a>>,
}

#[cfg(test)]
pub(crate) fn topology_streams<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a Scan<'_>,
) -> Result<Vec<&'a [u8]>, CodecError> {
    let semantic = prepare_topology_streams(ctx, scan)?;
    let mut bytes = ctx.collection_vec(semantic.len(), "nx topology byte views")?;
    for stream in semantic {
        bytes.push(stream.source.bytes());
    }
    Ok(bytes)
}

fn prepare_topology_streams<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a Scan<'_>,
) -> Result<Vec<TopologyStream<'a>>, CodecError> {
    let mut semantic = ctx.collection_vec(scan.streams.len(), "nx prepared topology streams")?;
    for (ordinal, stream) in scan.streams.iter().enumerate() {
        let (source, deltas) = if stream.kind() == StreamKind::Deltas {
            (
                RawSource::default(),
                Some(crate::records::DeltasRecords::read(ctx, &stream.inflated)?),
            )
        } else {
            (
                RawSource::raw(RawStreamOrdinal::new(ordinal), &stream.inflated),
                None,
            )
        };
        semantic.push(TopologyStream { source, deltas });
    }
    Ok(semantic)
}

/// Pair each eligible delta stream with the nearest preceding partition stream of the
/// same schema. `eligible_deltas`, when `Some`, restricts pairing to those delta
/// ordinals; when `None`, every delta stream is eligible.
pub(super) fn pair_stream_indices(
    ctx: &DecodeContext<'_>,
    streams: &[Stream],
    eligible_deltas: Option<&BTreeSet<usize>>,
) -> Result<BTreeMap<usize, Vec<usize>>, CodecError> {
    let mut pairs = BTreeMap::<usize, Vec<usize>>::new();
    for (delta, stream) in streams.iter().enumerate() {
        if stream.kind() != StreamKind::Deltas
            || eligible_deltas.is_some_and(|eligible| !eligible.contains(&delta))
        {
            continue;
        }
        let mut partition = None;
        for (ordinal, candidate) in streams[..delta].iter().enumerate().rev() {
            ctx.charge_work(1, "nx delta partition scan")?;
            if candidate.kind() == StreamKind::Partition
                && candidate.schema_token() == stream.schema_token()
            {
                partition = Some(ordinal);
                break;
            }
        }
        if let Some(partition) = partition {
            if !pairs.contains_key(&partition) {
                ctx.charge_collection_items(1, "nx delta pair partitions")?;
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(
                        std::mem::size_of::<(usize, Vec<usize>)>()
                    ),
                    "nx delta pair partitions",
                )?;
            }
            let deltas = pairs.entry(partition).or_default();
            ctx.reserve_vec(deltas, 1, "nx delta pair members")?;
            deltas.push(delta);
        }
    }
    Ok(pairs)
}

/// Cached primary topology and geometry. Deltas streams retain an empty view.
pub(crate) struct StreamView<'a> {
    /// Original stream bytes and their raw origin; empty for Deltas streams.
    pub(crate) source: RawSource<'a>,
    /// Topology record graph.
    pub(crate) graph: Rc<Graph>,
    /// Type-60 offset surfaces.
    pub(crate) offset_surfaces: Vec<OffsetSurface>,
    /// Type-56 rolling-ball blend surfaces.
    pub(crate) blend_surfaces: Vec<BlendSurface>,
    /// Type-133 trimmed curves.
    pub(crate) trimmed_curves: Vec<TrimmedCurve>,
    /// Type-137 surface curves.
    pub(crate) surface_curves: Vec<SurfaceCurve>,
    /// Intersection-construction scan.
    pub(crate) intersections: CurveScan,
}

impl<'a> StreamView<'a> {
    fn without_geometry(source: RawSource<'a>) -> Self {
        StreamView {
            source,
            graph: Rc::new(Graph::default()),
            offset_surfaces: Vec::new(),
            blend_surfaces: Vec::new(),
            trimmed_curves: Vec::new(),
            surface_curves: Vec::new(),
            intersections: CurveScan::default(),
        }
    }

    /// Parse cached primary families from their original stream bytes.
    fn parse_uniform(
        ctx: &DecodeContext<'_>,
        bytes: &'a [u8],
        point_layout: Option<crate::intersection::ChartPointLayout>,
        ordinal: RawStreamOrdinal,
    ) -> Result<Self, CodecError> {
        let graph = Rc::new(Graph::parse(ctx, bytes)?);
        let offset_surfaces = graph.offset_surfaces(ctx)?;
        let blend_surfaces = graph.blend_surfaces(ctx)?;
        let trimmed_curves = graph.trimmed_curves(ctx)?;
        let surface_curves = graph.surface_curves(ctx)?;
        let intersections = match point_layout {
            Some(layout) => intersection::scan_with_graph(ctx, bytes, &graph, layout)?,
            None => CurveScan::default(),
        };
        Ok(StreamView {
            source: RawSource::raw(ordinal, bytes),
            graph,
            offset_surfaces,
            blend_surfaces,
            trimmed_curves,
            surface_curves,
            intersections,
        })
    }
}

/// One stream's shared parses, raw source, and deferred geometry inputs.
struct StreamParse<'a> {
    view: StreamView<'a>,
    deltas: Option<crate::records::DeltasRecords<'a>>,
}

/// Every expensive per-stream Parasolid parse, once per distinct byte view,
/// indexed by stream ordinal. NURBS parsing remains deferred until requested.
pub(crate) struct ParsedStreams<'a> {
    streams: Vec<StreamParse<'a>>,
}

impl<'a> ParsedStreams<'a> {
    /// Parse primary geometry once and retain the Deltas owner for native consumers.
    pub(crate) fn parse(ctx: &DecodeContext<'_>, scan: &'a Scan) -> Result<Self, CodecError> {
        let mut topology = prepare_topology_streams(ctx, scan)?;
        let mut streams = ctx.collection_vec(scan.streams.len(), "nx parsed stream records")?;
        for (ordinal, stream) in scan.streams.iter().enumerate() {
            let source = std::mem::take(&mut topology[ordinal].source);
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<StreamView>()),
                "nx parsed stream view",
            )?;
            let view = match &stream.body {
                crate::parasolid::StreamBody::Parasolid { subtype, .. }
                    if stream.kind() != StreamKind::Deltas =>
                {
                    StreamView::parse_uniform(
                        ctx,
                        &stream.inflated,
                        subtype.chart_point_layout(),
                        RawStreamOrdinal::new(ordinal),
                    )?
                }
                _ => StreamView::without_geometry(source),
            };
            streams.push(StreamParse {
                view,
                deltas: topology[ordinal].deltas.take(),
            });
        }
        Ok(Self { streams })
    }

    pub(crate) fn deltas(&self, ordinal: usize) -> Option<&crate::records::DeltasRecords<'a>> {
        self.streams.get(ordinal)?.deltas.as_ref()
    }

    pub(super) fn take_deltas(
        &mut self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Vec<Option<crate::records::DeltasRecords<'a>>>, CodecError> {
        let mut owners = ctx.collection_vec(self.streams.len(), "nx Deltas owner slots")?;
        for stream in &mut self.streams {
            owners.push(stream.deltas.take());
        }
        Ok(owners)
    }

    /// The cached parses of the stream at `ordinal`.
    pub(crate) fn stream(&self, ordinal: usize) -> &StreamView<'a> {
        &self.streams[ordinal].view
    }

    /// Iterate `(ordinal, parses)` over every stream.
    pub(super) fn iter(&self) -> impl Iterator<Item = (usize, &StreamView<'a>)> {
        self.streams
            .iter()
            .enumerate()
            .map(|(ordinal, stream)| (ordinal, &stream.view))
    }

    /// Parse NURBS geometry for the selected semantic stream when requested.
    pub(crate) fn parse_nurbs(
        &self,
        ctx: &DecodeContext<'_>,
        ordinal: usize,
    ) -> Result<crate::nurbs::Parsed, CodecError> {
        let stream = &self.streams[ordinal];
        crate::nurbs::parse_with_graph(ctx, stream.view.source.bytes(), &stream.view.graph)
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::test_deltas::bspline_partition_stream;
    use cadmpeg_core::decode::{DecodeContext, ResourceDimension};

    use super::{topology_streams, ParsedStreams};

    fn one_delta_pair() -> Vec<crate::parasolid::Stream> {
        let stream = |subtype, file_offset| crate::parasolid::Stream {
            file_offset,
            consumed: 0,
            inflated: Vec::new(),
            body: crate::parasolid::StreamBody::Parasolid {
                subtype,
                schema: Some(
                    cadmpeg_parasolid::OwnedSchemaToken::try_from("SCH_PAIR")
                        .expect("the fixture text is a schema token"),
                ),
            },
        };
        vec![
            stream(crate::parasolid::ParasolidSubtype::Partition, 0),
            stream(crate::parasolid::ParasolidSubtype::Deltas, 1),
        ]
    }

    fn scan_with_streams(streams: Vec<crate::parasolid::Stream>) -> crate::decode::Scan<'static> {
        crate::decode::Scan {
            container: crate::container::Container {
                data: Vec::new().into(),
                physical_size: 0,
                layout: crate::container::test_modern_layout(0x06),
                entries: Vec::new(),
                fastload_table: None,
                indexed_section_layouts: std::sync::OnceLock::new(),
                om_section_cache: std::sync::OnceLock::new(),
            },
            streams,
        }
    }

    fn with_collection_limit<T>(
        max_collection_items: u64,
        f: impl FnOnce(&DecodeContext<'_>) -> T,
    ) -> T {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_collection_items = max_collection_items;
            },
            |ctx| f(ctx),
        )
    }

    #[test]
    fn topology_preparation_refuses_stream_slots_at_collection_limit() {
        let scan = scan_with_streams(vec![crate::parasolid::Stream {
            file_offset: 0,
            consumed: 0,
            inflated: Vec::new(),
            body: crate::parasolid::StreamBody::Preview,
        }]);
        let error = with_collection_limit(0, |ctx| topology_streams(ctx, &scan))
            .expect_err("the prepared stream needs one collection item");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "nx prepared topology streams"
        ));
    }

    #[test]
    fn topology_preparation_refuses_stream_slots_at_retained_limit() {
        let scan = scan_with_streams(vec![crate::parasolid::Stream {
            file_offset: 0,
            consumed: 0,
            inflated: Vec::new(),
            body: crate::parasolid::StreamBody::Preview,
        }]);

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_retained_bytes =
                    cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                        super::TopologyStream<'_>,
                    >()) - 1;
            },
            |ctx| {
                let error = topology_streams(ctx, &scan)
                    .expect_err("one prepared stream exceeds the retained limit");
                assert!(matches!(
                    error,
                    cadmpeg_core::CodecError::ResourceLimit(limit)
                        if limit.dimension == ResourceDimension::RetainedBytes
                            && limit.operation == "nx prepared topology streams"
                ));
            },
        );
    }

    #[test]
    fn topology_views_refuse_output_slots_at_collection_limit() {
        let scan = scan_with_streams(vec![crate::parasolid::Stream {
            file_offset: 0,
            consumed: 0,
            inflated: Vec::new(),
            body: crate::parasolid::StreamBody::Preview,
        }]);
        let error = with_collection_limit(1, |ctx| topology_streams(ctx, &scan))
            .expect_err("the output view needs a second collection item");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "nx topology byte views"
        ));
    }

    #[test]
    fn parsed_streams_refuse_record_slots_at_collection_limit() {
        let scan = scan_with_streams(vec![crate::parasolid::Stream {
            file_offset: 0,
            consumed: 0,
            inflated: Vec::new(),
            body: crate::parasolid::StreamBody::Preview,
        }]);
        let error = with_collection_limit(1, |ctx| ParsedStreams::parse(ctx, &scan))
            .err()
            .expect("the parsed record needs a second collection item");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "nx parsed stream records"
        ));
    }

    #[test]
    fn delta_census_transfer_refuses_slots_at_collection_limit() {
        let scan = scan_with_streams(vec![crate::parasolid::Stream {
            file_offset: 0,
            consumed: 0,
            inflated: Vec::new(),
            body: crate::parasolid::StreamBody::Preview,
        }]);
        let error = with_collection_limit(2, |ctx| {
            let mut parsed = ParsedStreams::parse(ctx, &scan)
                .expect("topology and parsed record use the two available slots");
            parsed.take_deltas(ctx)
        })
        .expect_err("census transfer needs a third collection item");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "nx Deltas owner slots"
        ));
    }

    fn pair_under_limits(collection_items: u64, work_units: u64) -> cadmpeg_core::CodecError {
        let streams = one_delta_pair();

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_collection_items = collection_items;
                policy.limits.max_work_units = work_units;
            },
            |ctx| {
                super::pair_stream_indices(ctx, &streams, None)
                    .expect_err("one delta pair exceeds the selected limit")
            },
        )
    }

    #[test]
    fn delta_pairing_refuses_partition_map_at_collection_limit() {
        assert!(matches!(
            pair_under_limits(0, u64::MAX),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "nx delta pair partitions"
        ));
    }

    #[test]
    fn delta_pairing_refuses_nested_member_at_collection_limit() {
        assert!(matches!(
            pair_under_limits(1, u64::MAX),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "nx delta pair members"
        ));
    }

    #[test]
    fn delta_pairing_refuses_partition_scan_at_work_limit() {
        assert!(matches!(
            pair_under_limits(u64::MAX, 0),
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "nx delta partition scan"
        ));
    }

    #[test]
    fn delta_pairing_charges_only_examined_partitions() {
        let mut streams = vec![crate::parasolid::Stream {
            file_offset: 10,
            consumed: 0,
            inflated: Vec::new(),
            body: crate::parasolid::StreamBody::Preview,
        }];
        streams.extend(one_delta_pair());

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_work_units = 1;
            },
            |ctx| {
                let pairs = super::pair_stream_indices(ctx, &streams, None)
                    .expect("the preceding partition matches within one work unit");
                assert_eq!(pairs, std::collections::BTreeMap::from([(1, vec![2])]));
            },
        );
    }

    #[test]
    fn unchanged_stream_views_borrow_the_inflated_bytes() {
        let scan = crate::decode::Scan {
            container: crate::container::Container {
                data: Vec::new().into(),
                physical_size: 0,
                layout: crate::container::test_modern_layout(0x06),
                entries: Vec::new(),
                fastload_table: None,
                indexed_section_layouts: std::sync::OnceLock::new(),
                om_section_cache: std::sync::OnceLock::new(),
            },
            streams: vec![crate::parasolid::Stream {
                file_offset: 0,
                consumed: 3,
                inflated: vec![1, 2, 3],
                body: crate::parasolid::StreamBody::Parasolid {
                    subtype: crate::parasolid::ParasolidSubtype::Partition,
                    schema: Some(
                        cadmpeg_parasolid::OwnedSchemaToken::try_from("SCH_TEST_1_9999")
                            .expect("the fixture text is a schema token"),
                    ),
                },
            }],
        };

        let topology = crate::test_support::with_decode_context(|ctx| {
            topology_streams(ctx, &scan).expect("test topology streams")
        });
        let parsed = crate::test_support::with_decode_context(|ctx| {
            ParsedStreams::parse(ctx, &scan).expect("test parsed streams")
        });

        assert!(std::ptr::eq(
            topology[0],
            scan.streams[0].inflated.as_slice()
        ));
        assert!(std::ptr::eq(
            parsed.streams[0].view.source.bytes(),
            scan.streams[0].inflated.as_slice()
        ));
    }

    #[test]
    fn nurbs_geometry_is_parsed_for_the_selected_semantic_stream() {
        let stream = |file_offset| {
            let inflated = bspline_partition_stream();
            crate::parasolid::Stream {
                file_offset,
                consumed: u64::try_from(inflated.len()).expect("test stream length fits u64"),
                inflated,
                body: crate::parasolid::StreamBody::Parasolid {
                    subtype: crate::parasolid::ParasolidSubtype::Partition,
                    schema: Some(
                        cadmpeg_parasolid::OwnedSchemaToken::try_from("SCH_TEST_1_9999")
                            .expect("the fixture text is a schema token"),
                    ),
                },
            }
        };
        let scan = crate::decode::Scan {
            container: crate::container::Container {
                data: Vec::new().into(),
                physical_size: 0,
                layout: crate::container::test_modern_layout(0x06),
                entries: Vec::new(),
                fastload_table: None,
                indexed_section_layouts: std::sync::OnceLock::new(),
                om_section_cache: std::sync::OnceLock::new(),
            },
            streams: vec![stream(0), stream(1)],
        };

        let parsed = crate::test_support::with_decode_context(|ctx| {
            ParsedStreams::parse(ctx, &scan).expect("test parsed streams")
        });

        let expected = crate::test_support::with_decode_context(|ctx| {
            crate::nurbs::parse_with_graph(
                ctx,
                parsed.streams[1].view.source.bytes(),
                &parsed.streams[1].view.graph,
            )
        })
        .unwrap();
        let actual =
            crate::test_support::with_decode_context(|ctx| parsed.parse_nurbs(ctx, 1)).unwrap();
        assert_eq!(actual.surfaces.len(), expected.surfaces.len());
        assert_eq!(actual.curves.len(), expected.curves.len());
        assert_eq!(actual.pcurves.len(), expected.pcurves.len());
        assert!(!actual.surfaces.is_empty());
        assert!(!actual.curves.is_empty());
        assert_eq!(
            actual
                .surfaces
                .iter()
                .map(|surface| surface.pos)
                .collect::<Vec<_>>(),
            expected
                .surfaces
                .iter()
                .map(|surface| surface.pos)
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            actual
                .curves
                .iter()
                .map(|curve| curve.pos)
                .collect::<Vec<_>>(),
            expected
                .curves
                .iter()
                .map(|curve| curve.pos)
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            actual
                .pcurves
                .iter()
                .map(|pcurve| pcurve.pos)
                .collect::<Vec<_>>(),
            expected
                .pcurves
                .iter()
                .map(|pcurve| pcurve.pos)
                .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn segment_order_pairs_delta_across_intervening_non_history_stream() {
        use crate::parasolid::Stream;
        use std::collections::BTreeSet;

        let stream = |subtype, schema: Option<&str>, file_offset| Stream {
            file_offset,
            consumed: 0,
            inflated: Vec::new(),
            body: crate::parasolid::StreamBody::Parasolid {
                subtype,
                schema: schema.map(|schema| {
                    cadmpeg_parasolid::OwnedSchemaToken::try_from(schema)
                        .expect("the fixture text is a schema token")
                }),
            },
        };
        let streams = vec![
            stream(
                crate::parasolid::ParasolidSubtype::Partition,
                Some("SCH_A"),
                10,
            ),
            Stream {
                file_offset: 20,
                consumed: 0,
                inflated: Vec::new(),
                body: crate::parasolid::StreamBody::Preview,
            },
            stream(
                crate::parasolid::ParasolidSubtype::Deltas,
                Some("SCH_A"),
                30,
            ),
            stream(
                crate::parasolid::ParasolidSubtype::Partition,
                Some("SCH_B"),
                40,
            ),
            stream(
                crate::parasolid::ParasolidSubtype::Deltas,
                Some("SCH_A"),
                50,
            ),
            stream(
                crate::parasolid::ParasolidSubtype::Deltas,
                Some("SCH_B"),
                60,
            ),
        ];
        let eligible = BTreeSet::from([2usize, 5]);
        assert_eq!(
            crate::test_support::with_decode_context(|ctx| {
                super::pair_stream_indices(ctx, &streams, Some(&eligible))
                    .expect("test delta pairing")
            }),
            std::collections::BTreeMap::from([(0, vec![2]), (3, vec![5])])
        );
    }
}
