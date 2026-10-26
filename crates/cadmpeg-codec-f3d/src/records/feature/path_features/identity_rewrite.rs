// SPDX-License-Identifier: Apache-2.0
//! Direct identity walks for native record fields.

use super::{
    DesignLoftConstruction, DesignPipeConstruction, DesignPipePointSelection, DesignRevolveConstruction,
    DesignSweepConstruction,
};

rewrite_native_record!(DesignLoftConstruction, []; {operation, operation_offset});
rewrite_native_record!(DesignPipeConstruction, [point_selection]; {operation, operation_offset, is_hollow, is_hollow_offset, post_operation_bytes, post_operation_offset, record_indexes});
rewrite_native_record!(DesignPipePointSelection, [point_data]; {prefix_reference_offset});
rewrite_native_scalar!(DesignRevolveConstruction);
rewrite_native_record!(DesignSweepConstruction, []; {operation, operation_offset, values, record_indexes, value_offsets});
