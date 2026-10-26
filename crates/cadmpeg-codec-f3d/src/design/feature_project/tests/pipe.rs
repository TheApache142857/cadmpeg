// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::scope::DesignScopePayload;
use crate::test_support::pipe::{Config, Fixture};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

#[test]
fn pipe_dispatch_preserves_native_facts_without_inventing_a_circle() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for raw in [[1, 1], [0, 1], [127, 254]] {
        for hollow in [false, true] {
            for operation in [1, 2, 3, 4] {
                for fractions in [[1.0, 0.0], [0.0, 1.0], [1.0, 1.0], [0.25, 0.75]] {
                    let native = Fixture::new(Config {
                        raw, hollow, operation, values: [fractions[0], fractions[1], 0.6, 0.35],
                        ..Default::default()
                    }).decode(&ctx).unwrap();
                    let scope = &native.design_parameter_scopes[0];
                    let DesignScopePayload::Pipe(Some(pipe)) = scope.payload() else { panic!("complete source fixture") };
                    assert_eq!(pipe.is_hollow, hollow);
                    assert_eq!(pipe.post_operation_bytes, raw);
                    let (features, _) = super::project_single_scope_with_context(&ctx, scope).unwrap();
                    assert_eq!(features.len(), 1);
                    assert!(matches!(features[0].definition(), FeatureDefinition::Operation(FeatureOperation::Native { kind, .. }) if kind == "Pipe"));
                    assert!(features[0].native_ref.is_some());
                    assert!(!serde_json::to_string(&features).unwrap().contains("circular_region"));
                    assert_eq!(native.design_parameter_owners.iter().find(|owner| owner.record_index() == pipe.record_indexes[2]).unwrap().evaluated_value().get(), 0.6);
                    assert_eq!(native.design_parameter_owners.iter().find(|owner| owner.record_index() == pipe.record_indexes[3]).unwrap().evaluated_value().get(), 0.35);
                }
            }
        }
    }
}

#[test]
fn pipe_native_dimensions_have_no_shape_specific_wall_predicate() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for thickness in [-1.0, 0.0, 0.35, 2.0] {
        for hollow in [false, true] {
            let native = Fixture::new(Config { hollow, values: [1.0, 0.0, 0.6, thickness], ..Default::default() }).decode(&ctx).unwrap();
            crate::records::feature::path_features::pipe::admit_native(&ctx, &native).unwrap();
            let (features, _) = super::project_single_scope_with_context(&ctx, &native.design_parameter_scopes[0]).unwrap();
            assert!(matches!(features[0].definition(), FeatureDefinition::Operation(FeatureOperation::Native { .. })));
        }
    }
}
