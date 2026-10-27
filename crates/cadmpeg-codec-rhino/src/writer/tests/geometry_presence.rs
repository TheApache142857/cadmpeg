// SPDX-License-Identifier: Apache-2.0
//! Whether a written 3DM archive holds geometry.

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::Point;

use crate::{RhinoArchiveVersion, RhinoCodec};

fn geometry_written(ir: &CadIr) -> bool {
    RhinoCodec
        .plan(
            EncodeInput::new(ir, None),
            TargetRequest::Explicit(RhinoArchiveVersion::V8.descriptor().id.as_str()),
        )
        .expect("the document writes")
        .report()
        .geometry_written()
}

/// The archive holds geometry when its object table holds an object.
#[test]
fn the_report_states_whether_the_object_table_holds_an_object() {
    let mut point = CadIr::empty();
    point.model.points.push(Point::new(
        "cadir:model:point#free".try_into().expect("identity"),
        FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).expect("finite"),
        None,
    ));
    assert!(geometry_written(&point));
    assert!(!geometry_written(&CadIr::empty()));
}
