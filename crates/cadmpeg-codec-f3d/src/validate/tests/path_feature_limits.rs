// SPDX-License-Identifier: Apache-2.0

fn path_error(max_items: u64, max_retained_bytes: u64) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use crate::records::feature::{
            extrude::DesignExtrudeOperation, path_features::DesignLoftConstruction,
            scope::DesignScopePayload,
        };
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = super::construction_group_limits::native(true, false);
        native.design_parameter_scopes[0]
            .try_edit(|draft| {
                draft.payload = DesignScopePayload::Loft(Some(DesignLoftConstruction {
                    operation: DesignExtrudeOperation::NewBody,
                    operation_offset: 0,
                }));
            })
            .unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained_bytes;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        let error = super::super::validate_path_feature_operand_roles(&ctx, &mut Vec::new()).unwrap_err();
        let cadmpeg_core::CodecError::ResourceLimit(limit) = &error else { panic!("typed resource refusal") };
        assert_eq!(decode.resource_refusal().as_ref(), Some(limit));
        assert!(matches!(decode.charge_work(1, "after path-feature refusal"), Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if &sticky == limit));
        error
    })
}

#[test]
fn path_feature_operand_group_refuses_collection_limit() {
    let error = path_error(0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D path-feature operand groups")
    );
}

#[test]
fn path_feature_operand_role_refuses_collection_limit() {
    let error = path_error(1, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D path-feature operand roles")
    );
}

#[test]
fn path_feature_invalid_finding_refuses_collection_limit() {
    let error = path_error(2, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn path_feature_invalid_entity_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D validation entity",
        |cap| Err::<(), cadmpeg_core::CodecError>(path_error(u64::MAX, cap)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}
