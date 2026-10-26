// SPDX-License-Identifier: Apache-2.0
//! Path features: revolve, loft, sweep and pipe constructions.

use super::extrude::DesignExtrudeOperation;
use super::work_geometry::DesignWorkPointConstruction;
use crate::records::identity::Located;
use cadmpeg_ir::scalar::{FiniteReal, PositiveAngle};
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(
    deserialize_opposite_angle_offset,
    u64,
    "opposite_angle_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_opposite_angle_record_index,
    u32,
    "opposite_angle_record_index"
);
/// Exact construction carried by a Revolve, Loft, Sweep, or Pipe scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum DesignPathFeatureConstruction {
    /// One-sided fixed-angle revolution result operation.
    Revolve(DesignRevolveConstruction),
    /// Loft result operation.
    Loft(DesignLoftConstruction),
    /// Sweep result operation and fixed dimension lanes.
    Sweep(DesignSweepConstruction),
    /// Native Pipe prefix and canonical scalar-owner identities.
    Pipe(DesignPipeConstruction),
}

/// Fixed construction of a `Revolve` scope.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignRevolveConstructionWire",
    into = "DesignRevolveConstructionWire"
)]
pub(crate) struct DesignRevolveConstruction {
    /// Boolean result operation.
    pub(crate) operation: DesignExtrudeOperation,
    /// Byte offset of the operation u32.
    pub(crate) operation_offset: u64,
    /// Positive angular travel in radians.
    pub(crate) angle: PositiveAngle,
    /// Referenced angular-travel scalar record.
    pub(crate) angle_record_index: u32,
    /// Byte offset of the angular-travel scalar.
    pub(crate) angle_offset: u64,
    /// Zero-valued opposite-side angle scalar record, when serialized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) opposite_angle: Option<Located<u32>>,
}

#[derive(Serialize, Deserialize)]
struct DesignRevolveConstructionWire {
    /// Boolean result operation.
    operation: DesignExtrudeOperation,
    /// Byte offset of the operation u32.
    operation_offset: u64,
    /// Positive angular travel in radians.
    angle: f64,
    /// Referenced angular-travel scalar record.
    angle_record_index: u32,
    /// Byte offset of the angular-travel scalar.
    angle_offset: u64,
    /// Zero-valued opposite-side angle scalar record, when serialized.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opposite_angle_record_index"
    )]
    opposite_angle_record_index: Option<u32>,
    /// Byte offset of the opposite-side angle scalar, when serialized.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opposite_angle_offset"
    )]
    opposite_angle_offset: Option<u64>,
}

impl From<DesignRevolveConstruction> for DesignRevolveConstructionWire {
    fn from(value: DesignRevolveConstruction) -> Self {
        Self {
            operation: value.operation,
            operation_offset: value.operation_offset,
            angle: value.angle.get(),
            angle_record_index: value.angle_record_index,
            angle_offset: value.angle_offset,
            opposite_angle_record_index: value.opposite_angle.map(|located| located.value),
            opposite_angle_offset: value.opposite_angle.map(|located| located.offset),
        }
    }
}

impl TryFrom<DesignRevolveConstructionWire> for DesignRevolveConstruction {
    type Error = String;
    fn try_from(value: DesignRevolveConstructionWire) -> Result<Self, Self::Error> {
        Ok(Self {
            operation: value.operation,
            operation_offset: value.operation_offset,
            angle: PositiveAngle::new(value.angle).ok_or("angle must be positive and finite")?,
            angle_record_index: value.angle_record_index,
            angle_offset: value.angle_offset,
            opposite_angle: match (
                value.opposite_angle_record_index,
                value.opposite_angle_offset,
            ) {
                (None, None) => None,
                (Some(value), Some(offset)) => Some(Located { value, offset }),
                _ => {
                    return Err(
                        "opposite_angle_record_index and opposite_angle_offset must occur together"
                            .into(),
                    )
                }
            },
        })
    }
}

/// Fixed construction of a `Loft` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignLoftConstruction {
    /// Boolean result operation.
    pub(crate) operation: DesignExtrudeOperation,
    /// Byte offset of the operation u32.
    pub(crate) operation_offset: u64,
}

/// Fixed construction of a `Sweep` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignSweepConstruction {
    /// Boolean result operation.
    pub(crate) operation: DesignExtrudeOperation,
    /// Byte offset of the operation u32.
    pub(crate) operation_offset: u64,
    /// Six scalar values in `AlongDistance`, `AgainstDistance`,
    /// `AlongRailDistance`, `AgainstRailDistance`, `TwistAngle`, and `TaperAngle` order.
    pub(crate) values: [FiniteReal; 6],
    /// Referenced scalar records in lane order.
    pub(crate) record_indexes: [u32; 6],
    /// Byte offsets of the scalar values in lane order.
    pub(crate) value_offsets: [u64; 6],
}

/// Native construction of a `Pipe` scope.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "DesignPipeConstructionWire")]
pub(crate) struct DesignPipeConstruction {
    /// Boolean result operation.
    pub(crate) operation: DesignExtrudeOperation,
    /// Byte offset of the operation u32.
    pub(crate) operation_offset: u64,
    /// Whether the section has an inward wall.
    pub(crate) is_hollow: bool,
    /// Byte offset of the hollow flag.
    pub(crate) is_hollow_offset: u64,
    /// Two uninterpreted bytes following the result operation.
    pub(crate) post_operation_bytes: cadmpeg_ir::native::bytes::NativeBytes<[u8; 2]>,
    /// Byte offset of the first uninterpreted byte.
    pub(crate) post_operation_offset: u64,
    /// Canonical scalar owners in fraction, reverse fraction, size, thickness order.
    pub(crate) record_indexes: [u32; 4],
    /// Optional native point-selection relation.
    pub(crate) point_selection: Option<DesignPipePointSelection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignPipePointSelection {
    pub(crate) prefix_reference_offset: u64,
    pub(crate) point_data: DesignWorkPointConstruction,
}

cadmpeg_core::named_optional_field!(deserialize_pipe_point_selection, DesignPipePointSelection, "point_selection");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DesignPipeConstructionWire {
    operation: DesignExtrudeOperation,
    operation_offset: u64,
    is_hollow: bool,
    is_hollow_offset: u64,
    post_operation_bytes: cadmpeg_ir::native::bytes::NativeBytes<[u8; 2]>,
    post_operation_offset: u64,
    record_indexes: [u32; 4],
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "deserialize_pipe_point_selection")]
    point_selection: Option<DesignPipePointSelection>,
}

impl Serialize for DesignPipeConstruction {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct BorrowedWire<'a> {
            operation: DesignExtrudeOperation,
            operation_offset: u64,
            is_hollow: bool,
            is_hollow_offset: u64,
            post_operation_bytes: cadmpeg_ir::native::bytes::NativeBytes<&'a [u8]>,
            post_operation_offset: u64,
            record_indexes: &'a [u32; 4],
            #[serde(skip_serializing_if = "Option::is_none")]
            point_selection: Option<&'a DesignPipePointSelection>,
        }
        BorrowedWire {
            operation: self.operation,
            operation_offset: self.operation_offset,
            is_hollow: self.is_hollow,
            is_hollow_offset: self.is_hollow_offset,
            post_operation_bytes: cadmpeg_ir::native::bytes::NativeBytes::from(&self.post_operation_bytes),
            post_operation_offset: self.post_operation_offset,
            record_indexes: &self.record_indexes,
            point_selection: self.point_selection.as_ref(),
        }.serialize(serializer)
    }
}

impl TryFrom<DesignPipeConstructionWire> for DesignPipeConstruction {
    type Error = String;

    fn try_from(value: DesignPipeConstructionWire) -> Result<Self, Self::Error> {
        if value.is_hollow_offset.checked_add(1) != Some(value.operation_offset)
            || value.operation_offset.checked_add(4) != Some(value.post_operation_offset)
            || value.record_indexes.iter().enumerate().any(|(ordinal, id)| {
                *id == 0 || value.record_indexes[..ordinal].contains(id)
            })
        {
            return Err("invalid Pipe prefix offsets or scalar identities".into());
        }
        Ok(Self {
            operation: value.operation,
            operation_offset: value.operation_offset,
            is_hollow: value.is_hollow,
            is_hollow_offset: value.is_hollow_offset,
            post_operation_bytes: value.post_operation_bytes,
            post_operation_offset: value.post_operation_offset,
            record_indexes: value.record_indexes,
            point_selection: value.point_selection,
        })
    }
}

impl DesignPipeConstruction {
    pub(super) fn scope_offsets_are_valid(&self, start: u64, table: u64) -> bool {
        use crate::layout::{compact_pipe_operation_prefix as compact, legacy_pipe_operation_prefix as legacy};
        let compact = start.checked_add(cadmpeg_core::decode::u64_from_index(compact::IS_HOLLOW));
        let legacy = start.checked_add(cadmpeg_core::decode::u64_from_index(legacy::IS_HOLLOW));
        if ![compact, legacy].contains(&Some(self.is_hollow_offset))
            || self.is_hollow_offset.checked_add(1) != Some(self.operation_offset)
            || self.operation_offset.checked_add(4) != Some(self.post_operation_offset)
            || self.post_operation_offset.checked_add(2).is_none_or(|end| end > table)
            || self.point_selection.as_ref().is_some_and(|selection| {
                self.post_operation_offset.checked_add(2).is_none_or(|end| selection.prefix_reference_offset <= end)
                    || selection.prefix_reference_offset.checked_add(10).is_none_or(|end| end > table)
            })
        {
            return false;
        }
        true
    }
}

pub(crate) mod pipe;
mod identity_rewrite;
