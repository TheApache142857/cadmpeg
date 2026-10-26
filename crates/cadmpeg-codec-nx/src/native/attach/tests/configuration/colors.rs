// SPDX-License-Identifier: Apache-2.0

use crate::native::om::display_color::DisplayColorFrame;
use crate::native::om::display_color::RmDisplayColorAssignment;
use crate::native::om::display_color::RmDisplayColorAssignmentEncoding;
use crate::om::column_row::TargetRow;
use crate::om::compact::CompactIndexAtom;

use crate::native::attach::ensure_rm_color_appearance;
use crate::native::attach::resolve_rm_source_color_bindings;
use crate::native::attach::RmSourceColorBinding;
use std::collections::BTreeMap;

fn rm_appearance_result(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
    twice: bool,
) -> Result<(), cadmpeg_core::CodecError> {
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            let definition = crate::native::om::PartColorDefinition {
                id: "nx:test:color#201".into(),
                color_table: "nx:test:table#0".into(),
                color_index: crate::om::color::PaletteIndex::new(201).unwrap(),
                name: "Iron Gray".into(),
                components: [(0.25_f64, 11), (0.5, 12), (0.75, 13)].map(|(value, offset)| {
                    let mut raw = (value * 4.0).to_be_bytes();
                    raw[0] -= 0x10;
                    (
                        crate::om::color::ColorComponent::read(&raw).unwrap(),
                        offset,
                    )
                }),
                source_offset: 10,
            };
            let mut ir = cadmpeg_ir::document::CadIr::empty();
            let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
            let mut appearances = BTreeMap::new();
            let mut reservation = ctx.reserve_scoped(0, "NX RM appearance identity lookup")?;
            let stream = cadmpeg_ir::annotations::StreamHandle::new(
                &cadmpeg_test_support::service_decode_context(),
                cadmpeg_ir::stream_name!("nx:container"),
                "fixture stream handle",
            )
            .unwrap();
            ensure_rm_color_appearance(
                ctx,
                &mut ir,
                &mut annotations,
                &mut appearances,
                &mut reservation,
                &definition,
                &stream,
            )?;
            if twice {
                ensure_rm_color_appearance(
                    ctx,
                    &mut ir,
                    &mut annotations,
                    &mut appearances,
                    &mut reservation,
                    &definition,
                    &stream,
                )?;
                assert_eq!(ir.model.appearances.len(), 1);
            }
            Ok(())
        },
    )
}

#[test]
fn rm_appearance_refuses_collection_limit() {
    let error =
        rm_appearance_result(|policy| policy.limits.max_collection_items = 0, false).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn rm_appearance_refuses_retained_limit() {
    let error =
        rm_appearance_result(|policy| policy.limits.max_retained_bytes = 0, false).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn rm_appearance_refuses_scoped_limit() {
    let error =
        rm_appearance_result(|policy| policy.limits.max_materialized_bytes = 0, false).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn rm_appearance_refuses_work_limit() {
    let error = rm_appearance_result(|policy| policy.limits.max_work_units = 0, true).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

#[test]
fn rm_source_color_bindings_require_one_palette_per_source_identity() {
    let assignment = |id: &str, source_id: Option<&str>, color_definition: &str, offset| {
        let color_index = crate::om::color::PaletteIndex::new(201).unwrap();
        RmDisplayColorAssignment {
            id: id.into(),
            ordinal: 0,
            frame: DisplayColorFrame::new(
                RmDisplayColorAssignmentEncoding::Target(
                    crate::om::column_row::TargetRow::<(), u64>::new(
                        CompactIndexAtom::read(&[7]).unwrap().into(),
                        [1, 2, 3].map(|value| CompactIndexAtom::read(&[value]).unwrap().into()),
                        crate::om::discriminators::IndexRowMode::Form04,
                        offset + u64::from(color_index.display_byte_len()),
                    )
                    .unwrap(),
                ),
                color_index,
            )
            .unwrap(),
            target_object_id: source_id.map(str::to_owned),
            color_definition: color_definition.into(),
            source_entry: "/Root/FastLoad/RMFastLoad".into(),
        }
    };
    let assignments = [
        assignment("assignment-b", Some("source-a"), "color-a", 20),
        assignment("assignment-a", Some("source-a"), "color-a", 10),
        assignment("assignment-c", Some("source-b"), "color-a", 30),
        assignment("assignment-d", Some("source-c"), "color-a", 40),
        assignment("assignment-e", Some("source-c"), "color-b", 50),
        assignment("assignment-f", None, "color-a", 60),
    ];
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| resolve_rm_source_color_bindings(
            ctx,
            &assignments
        ))
        .expect("admitted RM source colors"),
        vec![
            RmSourceColorBinding {
                source_id: "source-a".into(),
                color_definition: "color-a".into(),
                source_offset: 10,
            },
            RmSourceColorBinding {
                source_id: "source-b".into(),
                color_definition: "color-a".into(),
                source_offset: 30,
            },
        ]
    );
}

fn source_color_binding_result(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> Result<Vec<RmSourceColorBinding>, cadmpeg_core::CodecError> {
    let assignment = RmDisplayColorAssignment {
        id: "nx:test:assignment#0".into(),
        ordinal: 0,
        frame: DisplayColorFrame::new(
            RmDisplayColorAssignmentEncoding::Target(
                TargetRow::<(), u64>::new(
                    CompactIndexAtom::read(&[7]).unwrap().into(),
                    [1, 2, 3].map(|value| CompactIndexAtom::read(&[value]).unwrap().into()),
                    crate::om::discriminators::IndexRowMode::Form04,
                    22,
                )
                .unwrap(),
            ),
            crate::om::color::PaletteIndex::new(201).unwrap(),
        )
        .unwrap(),
        target_object_id: Some("nx:test:object-id#7".into()),
        color_definition: "nx:test:color#201".into(),
        source_entry: "/Root/FastLoad/RMFastLoad".into(),
    };

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| resolve_rm_source_color_bindings(ctx, &[assignment]),
    )
}

#[test]
fn rm_source_color_bindings_refuse_collection_limit() {
    let error =
        source_color_binding_result(|policy| policy.limits.max_collection_items = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn rm_source_color_bindings_refuse_retained_limit() {
    let error =
        source_color_binding_result(|policy| policy.limits.max_retained_bytes = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn rm_source_color_bindings_refuse_scoped_limit() {
    let error =
        source_color_binding_result(|policy| policy.limits.max_materialized_bytes = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn rm_source_color_bindings_refuse_work_limit() {
    let error = source_color_binding_result(|policy| policy.limits.max_work_units = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}
