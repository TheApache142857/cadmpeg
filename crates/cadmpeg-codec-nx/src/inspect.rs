// SPDX-License-Identifier: Apache-2.0
//! NX inspection summary and its resource-admitted output records.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use cadmpeg_core::container::{CompressionMethod, ContainerRole, EntryStorage, VerbatimLabel};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::ContainerSummary;

use crate::framing::node_kind::NodeKind;
use crate::{container, decode, parasolid, topology};

/// Build the container summary: one entry per catalogued directory stream, plus
/// one per embedded Parasolid stream, and the shared container notes.
pub(super) fn summarize(
    ctx: &DecodeContext<'_>,
    scan: &decode::Scan,
) -> Result<ContainerSummary, CodecError> {
    let entry_count = scan
        .container
        .entries
        .len()
        .checked_add(scan.streams.len())
        .ok_or_else(|| ctx.refuse_codec_limit("nx summary entries", 0, u64::MAX))?;
    let mut entries = ctx.collection_vec(entry_count, "nx summary entries")?;

    for entry in &scan.container.entries {
        let mut attributes = BTreeMap::new();
        insert_summary_attribute(
            ctx,
            &mut attributes,
            "region",
            "",
            false,
            SummaryValue::Text(entry.region.label()),
        )?;
        let storage = match entry.file_span() {
            Some((off, size)) => {
                insert_summary_attribute(
                    ctx,
                    &mut attributes,
                    "file_offset",
                    "",
                    false,
                    SummaryValue::Number(off),
                )?;
                EntryStorage::verbatim(VerbatimLabel::None, size)
            }
            None => {
                insert_summary_attribute(
                    ctx,
                    &mut attributes,
                    "kind",
                    "",
                    false,
                    SummaryValue::Text("directory"),
                )?;
                EntryStorage::Directory
            }
        };
        entries.push(ContainerEntry {
            name: ctx
                .format_retained(format_args!("{}", entry.name), "nx summary directory name")?,
            role: entry.content().role(),
            storage,
            attributes,
        });
    }

    let mut storage_notes_storage = ctx.reserve_scoped(0, "nx temporary storage notes")?;
    let mut storage_notes: Vec<String> = Vec::new();
    for (si, stream) in scan.streams.iter().enumerate() {
        let mut attributes = BTreeMap::new();
        insert_summary_attribute(
            ctx,
            &mut attributes,
            "file_offset",
            "",
            false,
            SummaryValue::Number(cadmpeg_core::decode::u64_from_index(stream.file_offset)),
        )?;
        insert_summary_attribute(
            ctx,
            &mut attributes,
            "kind",
            "",
            false,
            SummaryValue::Text(stream.kind().label()),
        )?;
        if let Some(schema) = stream.schema_token() {
            insert_summary_attribute(
                ctx,
                &mut attributes,
                "schema",
                "",
                false,
                SummaryValue::Text(schema.value()),
            )?;
        }
        if stream.kind().is_parasolid() && stream.kind() != parasolid::StreamKind::Deltas {
            let graph = topology::Graph::parse(ctx, &stream.inflated)?;
            for (kind, name) in [
                (NodeKind::Body, "body"),
                (NodeKind::Shell, "shell"),
                (NodeKind::Face, "face"),
                (NodeKind::Loop, "loop"),
                (NodeKind::Edge, "edge"),
                (NodeKind::Fin, "fin"),
                (NodeKind::Vertex, "vertex"),
                (NodeKind::Region, "region"),
            ] {
                let count = cadmpeg_core::decode::u64_from_index(graph.kind_count(kind));
                insert_summary_attribute(
                    ctx,
                    &mut attributes,
                    "records.",
                    name,
                    false,
                    SummaryValue::Number(count),
                )?;
                if stream.kind() == parasolid::StreamKind::Partition {
                    insert_summary_attribute(
                        ctx,
                        &mut attributes,
                        "records.live.",
                        name,
                        false,
                        SummaryValue::Number(count),
                    )?;
                }
            }
        } else if stream.kind() == parasolid::StreamKind::Deltas {
            let owner = crate::records::DeltasRecords::read(ctx, &stream.inflated)?;
            let census = owner.census();
            for (key, count) in [
                ("records.delta.instances", census.instance_count()),
                ("records.delta.schemas", census.schema_count()),
                ("records.delta.bytes_decoded", census.bytes_decoded()),
            ] {
                insert_summary_attribute(
                    ctx,
                    &mut attributes,
                    key,
                    "",
                    false,
                    SummaryValue::Number(cadmpeg_core::decode::u64_from_index(count)),
                )?;
            }
            insert_summary_attribute(
                ctx,
                &mut attributes,
                "records.delta.selection",
                "",
                false,
                SummaryValue::Text("unresolved"),
            )?;
            for (family, count) in census.full_counts(ctx)? {
                insert_summary_attribute(
                    ctx,
                    &mut attributes,
                    "records.delta.full.",
                    family,
                    true,
                    SummaryValue::Number(cadmpeg_core::decode::u64_from_index(count)),
                )?;
            }
        }
        let inflated_len = cadmpeg_core::decode::u64_from_index(stream.inflated.len());
        let storage = match scan.container.layout {
            container::ContainerLayout::Modern { .. } => EntryStorage::Compressed {
                method: CompressionMethod::Zlib,
                stored: None,
                expanded: Some(inflated_len),
            },
            container::ContainerLayout::LegacyCfb { .. } => {
                match EntryStorage::framed(VerbatimLabel::Stored, inflated_len, stream.consumed) {
                    Ok(storage) => storage,
                    Err(message) => {
                        storage_notes_storage.with_storage(|| {
                            ctx.reserve_vec(&mut storage_notes, 1, "nx summary storage notes")
                        })?;
                        storage_notes.push(ctx.format_retained(
                            format_args!(
                                "parasolid#{si}: {message}: {}/{inflated_len}",
                                stream.consumed
                            ),
                            "nx summary storage note",
                        )?);
                        EntryStorage::payload_only(VerbatimLabel::Stored, inflated_len)
                    }
                }
            }
        };

        entries.push(ContainerEntry {
            name: ctx.format_retained(format_args!("parasolid#{si}"), "nx summary stream name")?,
            role: if stream.kind().is_parasolid() {
                ContainerRole::ParasolidStream
            } else {
                ContainerRole::Preview
            },
            storage,
            attributes,
        });
    }

    let (classification, mut notes) = crate::scan_notes::summarize(ctx, scan)?;
    ctx.reserve_vec(
        &mut notes,
        storage_notes.len(),
        "nx combined inspection notes",
    )?;
    notes.extend(storage_notes);
    let container_kind = classification.container_kind();
    let (dialects, dialect_losses) = classification.into_report_parts();
    Ok(ContainerSummary::classified(
        dialects,
        container_kind,
        entries,
        dialect_losses,
        notes,
    ))
}

#[derive(Clone, Copy)]
enum SummaryValue<'a> {
    Text(&'a str),
    Number(u64),
}

fn decimal_len(number: u64) -> usize {
    if number == 0 {
        1
    } else {
        cadmpeg_core::decode::index_from_u32(number.ilog10() + 1)
    }
}

fn insert_summary_attribute(
    ctx: &DecodeContext<'_>,
    attributes: &mut BTreeMap<String, String>,
    prefix: &str,
    suffix: &str,
    lowercase_suffix: bool,
    value: SummaryValue<'_>,
) -> Result<(), CodecError> {
    let key_len = prefix
        .len()
        .checked_add(suffix.len())
        .ok_or_else(|| ctx.refuse_codec_limit("nx summary attribute text", 0, u64::MAX))?;
    let value_len = match value {
        SummaryValue::Text(text) => text.len(),
        SummaryValue::Number(number) => decimal_len(number),
    };

    let mut key = String::new();
    ctx.try_reserve_retained_text(&mut key, key_len, "nx summary attribute text")?;
    key.push_str(prefix);
    if lowercase_suffix {
        for character in suffix.chars() {
            key.push(character.to_ascii_lowercase());
        }
    } else {
        key.push_str(suffix);
    }
    let mut rendered = String::new();
    ctx.try_reserve_retained_text(&mut rendered, value_len, "nx summary attribute text")?;
    match value {
        SummaryValue::Text(text) => rendered.push_str(text),
        SummaryValue::Number(number) => write!(&mut rendered, "{number}")
            .map_err(|_| ctx.refuse_codec_limit("nx summary attribute text", 0, 1))?,
    }
    ctx.insert_btree_map(attributes, key, rendered, "nx summary attributes")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn deltas_inspection_counts_only_owned_instances() {
        let file = crate::test_support::test_prt::single_part_prt();
        crate::test_support::with_decode_context(|ctx| {
            let scan = crate::decode::Scan {
                container: crate::container::scan_bytes(ctx, file).unwrap(),
                streams: vec![crate::test_support::owned_deltas::stream(
                    crate::test_support::test_pmarks::complete(
                        crate::test_support::owned_deltas::body_stream(36),
                    ),
                )],
            };
            let summary = super::summarize(ctx, &scan).unwrap();
            let attributes = &summary.entries.last().unwrap().attributes;
            assert_eq!(
                attributes
                    .get("records.delta.instances")
                    .map(String::as_str),
                Some("1")
            );
            assert_eq!(
                attributes
                    .get("records.delta.full.body")
                    .map(String::as_str),
                Some("1")
            );
            assert!(!attributes.contains_key("records.body"));
            assert!(!attributes.contains_key("records.live.body"));
        });
    }

    #[test]
    fn inspection_summary_refuses_unadmitted_stream_count_work() {
        let file = crate::test_support::test_prt::single_part_prt();
        let container =
            crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, file))
                .unwrap();
        let scan = crate::decode::Scan {
            container,
            streams: vec![crate::parasolid::Stream {
                file_offset: 0,
                consumed: 0,
                inflated: Vec::new(),
                body: crate::parasolid::StreamBody::Preview,
            }],
        };
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = 0,
            |ctx| {
                let error = super::summarize(ctx, &scan).unwrap_err();
                assert!(
                    matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                        && limit.operation == "count NX streams")
                );
            },
        );
    }
}
