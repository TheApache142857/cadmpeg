// SPDX-License-Identifier: Apache-2.0
//! Exact source framing and complete native Pipe binding.

use super::point_data::exact_point_data_construction;
use super::shared_frames::{exact_fixed_scalar, extrude_operation_at, marked_record_reference};
use crate::container::ContainerScan;
use crate::design::decode::meta::{design_primary_frames, metadata_for_bulk_stream, DesignPrimaryFrame};
use crate::design::decode::sketch::{native_scope_charged, IndexedRecordOffsets};
use crate::ids::design_segment;
use crate::layout::{compact_pipe_operation_prefix as compact, legacy_pipe_operation_prefix as legacy};
use crate::native::F3dNative;
use crate::records::feature::path_features::{DesignPipeConstruction, DesignPipePointSelection};
use crate::records::feature::path_features::pipe::{self, PipeIndex};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut};
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use std::collections::HashMap;

pub(crate) fn bind_constructions(ctx: &DecodeContext<'_>, scan: &ContainerScan<'_>, native: &mut F3dNative) -> Result<(), CodecError> {
    if !native.design_parameter_scopes.iter().any(|scope| scope.kind() == DesignFeatureKind::Pipe) { return Ok(()) }
    let mut scratch = ctx.reserve_scoped(0, "F3D Pipe binding scratch")?;
    let bindings = scratch.with_storage(|| {
        let index = PipeIndex::new(ctx, native)?;
        let mut bindings = Vec::new();
        for entry in scan.entries.iter().filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream)) {
            let stream = native_scope_charged(ctx, &entry.name)?;
            let Some(meta) = metadata_for_bulk_stream(ctx, scan, &entry.name)? else { continue };
            let bytes = scan.entry_bytes(&entry.name)?;
            let frames = match design_primary_frames(ctx, bytes, &meta) {
                Ok(frames) => frames,
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Err(_) => continue,
            };
            let mut types = HashMap::new();
            for data in &meta.types {
                for entity in data.entities.values() {
                    ctx.reserve_map(&mut types, 1, "index F3D Pipe source type")?;
                    match types.entry(*entity) {
                        std::collections::hash_map::Entry::Vacant(entry) => { entry.insert(Some(data)); }
                        std::collections::hash_map::Entry::Occupied(mut entry) => { entry.insert(None); }
                    }
                }
            }
            let mut indexed = HashMap::new();
            for frame in &frames {
                if types.get(&frame.entity_id).and_then(|data| *data).is_none_or(|data| !std::ptr::eq(data, frame.design_type)) { continue }
                ctx.reserve_map(&mut indexed, 1, "index F3D Pipe source frame")?;
                match indexed.entry(frame.entity_id) {
                    std::collections::hash_map::Entry::Vacant(entry) => { entry.insert(Some(frame)); }
                    std::collections::hash_map::Entry::Occupied(mut entry) => { entry.insert(None); }
                }
            }
            let records = IndexedRecordOffsets::build(ctx, bytes)?;
            for (ordinal, scope) in native.design_parameter_scopes.iter().enumerate().filter(|(_, scope)| {
                scope.kind() == DesignFeatureKind::Pipe && crate::ids::native_stream(&scope.id) == Some(stream.as_str())
            }) {
                if let Some(construction) = read_construction(ctx, bytes, scope, &indexed, &records, &index)? {
                    ctx.push_vec(&mut bindings, (ordinal, construction), "retain F3D Pipe binding")?;
                }
            }
        }
        Ok::<_, CodecError>(bindings)
    })?;
    for (ordinal, construction) in bindings {
        if let Some(selection) = &construction.point_selection {
            let capacity = match selection.point_data.rule.form() {
                crate::records::feature::work_geometry::DesignWorkPointRuleForm::Native { inputs, .. } => inputs.capacity(),
                _ => 0,
            };
            ctx.charge_retained(
                u64_from_index(capacity).checked_mul(u64_from_index(std::mem::size_of::<crate::records::feature::work_geometry::DesignWorkPointInput>()))
                    .ok_or_else(|| ctx.refuse_codec_limit("retain F3D Pipe point inputs", 0, 1))?,
                "retain F3D Pipe point inputs",
            )?;
        }
        if let DesignScopePayloadMut::Pipe(slot) = native.design_parameter_scopes[ordinal].payload_mut() { *slot = Some(construction); }
    }
    Ok(())
}

fn read_prefix(bytes: &[u8], scope: &DesignParameterScope) -> Option<DesignPipeConstruction> {
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let table = usize::try_from(scope.reference_count_offset()).ok()?;
    let prefix = bytes.get(start..table)?;
    if prefix.get(compact::ZERO_RUN_8..compact::LEADING_PRESENCE) != Some(&[0; 8]) { return None }
    let (hollow, operation, raw) = match prefix.get(compact::LEADING_PRESENCE..=compact::PROPERTY_PRESENCE)? {
        [0, 0] => (compact::IS_HOLLOW, compact::OPERATION, compact::POST_OPERATION_BYTES),
        [0, 1] if View::u32_le_at(prefix, legacy::PROPERTY_COUNT) == Some(0) => (legacy::IS_HOLLOW, legacy::OPERATION, legacy::POST_OPERATION_BYTES),
        _ => return None,
    };
    let is_hollow = match prefix.get(hollow)? { 0 => false, 1 => true, _ => return None };
    Some(DesignPipeConstruction {
        operation: extrude_operation_at(prefix, operation)?,
        operation_offset: scope.byte_offset().checked_add(u64_from_index(operation))?,
        is_hollow,
        is_hollow_offset: scope.byte_offset().checked_add(u64_from_index(hollow))?,
        post_operation_bytes: prefix.get(raw..raw.checked_add(2)?)?.try_into().ok()?,
        post_operation_offset: scope.byte_offset().checked_add(u64_from_index(raw))?,
        record_indexes: [0; 4], point_selection: None,
    })
}

fn read_construction(
    ctx: &DecodeContext<'_>, bytes: &[u8], scope: &DesignParameterScope,
    frames: &HashMap<u64, Option<&DesignPrimaryFrame<'_>>>, records: &IndexedRecordOffsets,
    index: &PipeIndex<'_>,
) -> Result<Option<DesignPipeConstruction>, CodecError> {
    let frame = |id: u32| frames.get(&u64::from(id)).and_then(|frame| *frame);
    let Some(own) = frame(scope.record_index) else { return Ok(None) };
    if !pipe::is_pipe(own.design_type) || u64_from_index(own.start) != scope.byte_offset()
        || u64_from_index(own.member_end) != scope.paired_byte_offset() || own.class_tag != scope.class_tag {
        return Ok(None);
    }
    let Some(segment) = design_segment(&scope.id) else { return Ok(None) };
    let Some(mut construction) = read_prefix(bytes, scope) else { return Ok(None) };
    let mut lanes = 0_usize;
    let mut point = None;
    for id in scope.reference_members().values().copied() {
        ctx.charge_work(1, "bind F3D Pipe source reference")?;
        let Some(selected) = frame(id) else { return Ok(None) };
        if pipe::is_scalar(selected.design_type) {
            let Some(scalar) = exact_fixed_scalar(bytes, records, id) else { return Ok(None) };
            let Some(owner) = index.owner(segment, id) else { return Ok(None) };
            if lanes >= 4 || scalar.ordinal != u8::try_from(lanes).map_err(|_| ctx.refuse_codec_limit("F3D Pipe lane ordinal", 0, 1))?
                || scalar.owner_record_index != Some(scope.record_index)
                || scalar.start != u64_from_index(selected.start) || scalar.end != u64_from_index(selected.member_end)
                || scalar.value.get().to_bits() != owner.evaluated_value().get().to_bits() || scalar.value_offset != owner.evaluated_value_offset()
                || owner.byte_offset() != scalar.start || owner.byte_offset().checked_add(owner.frame_length()) != Some(scalar.end)
                || owner.class_tag() != &selected.class_tag || owner.local_ordinal() != u32::from(scalar.ordinal)
                || owner.scope_record_index() != scope.record_index { return Ok(None) }
            construction.record_indexes[lanes] = id;
            lanes += 1;
        } else if pipe::is_point(selected.design_type) {
            if point.replace((id, selected)).is_some() { return Ok(None) }
        }
    }
    if lanes != 4 { return Ok(None) }
    for group in index.groups(segment, scope.record_index) {
        let Some(selected) = frame(group.record_index) else { return Ok(None) };
        if u64_from_index(selected.start) != group.byte_offset || u64_from_index(selected.member_end) != group.paired_byte_offset
            || selected.class_tag != group.class_tag { return Ok(None) }
    }
    if let Some((id, selected)) = point {
        let mut types = HashMap::new();
        ctx.reserve_map(&mut types, 1, "index F3D Pipe point type")?;
        types.insert(u64::from(id), (selected.design_type.type_guid.as_str(), selected.design_type.version));
        let Some(point_data) = exact_point_data_construction(ctx, bytes, records, std::iter::once(&id), &types)? else { return Ok(None) };
        if point_data.point_record_byte_offset != u64_from_index(selected.start)
            || !records.frames(id).any(|(start, end)| start == selected.start && end == selected.member_end) { return Ok(None) }
        let mut prefix_reference_offset = None;
        let start = usize::try_from(construction.post_operation_offset).ok().and_then(|start| start.checked_add(2));
        let end = usize::try_from(scope.reference_count_offset()).ok();
        let (Some(start), Some(end)) = (start, end) else { return Ok(None) };
        for at in start..end {
            ctx.charge_work(1, "find F3D Pipe point prefix reference")?;
            if at.checked_add(11).is_some_and(|after| after <= end) && marked_record_reference(bytes, at) == Some(id) {
                if prefix_reference_offset.replace(u64_from_index(at + 1)).is_some() { return Ok(None) }
            }
        }
        let Some(prefix_reference_offset) = prefix_reference_offset else { return Ok(None) };
        construction.point_selection = Some(DesignPipePointSelection { prefix_reference_offset, point_data });
    }
    Ok(index.check_relation(ctx, scope, &construction)?.map(|_| construction))
}

#[cfg(test)]
mod tests;
