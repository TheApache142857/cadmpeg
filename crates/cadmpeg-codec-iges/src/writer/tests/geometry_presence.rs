// SPDX-License-Identifier: Apache-2.0
//! Whether a written IGES file holds geometry.

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::examples::unit_cube;

use crate::writer::{writes_geometry, Entity, EntityStatus};
use crate::{IgesCodec, IgesVersion};

fn entity(type_code: u32, status: EntityStatus) -> Entity {
    Entity {
        type_code,
        form: 0,
        label: "TEST",
        status,
        parameter_body: Vec::new(),
        transform: None,
    }
}

/// Only an entity that is not physically dependent and is not a definition
/// is geometry of the file, and directions, transformations, colors, and
/// name properties are not geometry.
#[test]
fn only_an_independent_geometric_entity_is_written_geometry() {
    assert!(!writes_geometry(&[]));
    assert!(!writes_geometry(&[
        entity(123, EntityStatus::Independent),
        entity(124, EntityStatus::Independent),
        entity(314, EntityStatus::Definition),
        entity(406, EntityStatus::Independent),
    ]));
    assert!(!writes_geometry(&[entity(
        110,
        EntityStatus::PhysicallyDependent
    )]));
    assert!(!writes_geometry(&[entity(110, EntityStatus::Definition)]));
    assert!(writes_geometry(&[entity(110, EntityStatus::Independent)]));
    assert!(writes_geometry(&[entity(
        110,
        EntityStatus::LogicallyDependent
    )]));
}

#[test]
fn a_synthesized_cube_reports_written_geometry() {
    let ir = unit_cube().expect("unit cube fixture is admitted");
    let plan = IgesCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(IgesVersion::V5_3.descriptor().id.as_str()),
        )
        .expect("the cube writes");
    assert!(plan.report().geometry_written());
}
