// SPDX-License-Identifier: Apache-2.0
//! Cross-record admission of native Pipe constructions.

use super::DesignPipeConstruction;
use crate::ids::design_segment;
use crate::native::F3dNative;
use crate::records::decal::DesignRecordHeader;
use crate::records::entity_header::{SegmentType, SegmentTypeData};
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayload};
use crate::records::parameters::DesignParameterOwner;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};

pub(crate) const PIPE_GUID: &str = "73615646-8752-4EE6-AD02-0B90804B7CD7";
pub(crate) const PIPE_BASE: &str = "3FED63D9-CDC5-4334-A5C5-6C10271320D5";
pub(crate) const SCALAR_GUID: &str = "D91D429C-1BB5-4640-B49F-69AF14F463E9";
pub(crate) const SCALAR_BASE: &str = "B24D5130-8751-4272-8143-D230D8757873";
const SETTINGS_GUID: &str = "9414C87F-8EA3-465D-9F78-FF2FF5E9881F";
const SETTINGS_BASE: &str = "60F291CB-499B-4AB1-B0E5-340368671724";
pub(crate) const GROUP_GUID: &str = "2CA5A1CD-C99B-4C9B-91AF-57989148E841";
const GROUP_BASE: &str = "F8CD0BB3-2CE6-4236-8C1B-6D0638BF215F";
const BODY_GUID: &str = "9716F783-676D-42E0-93F9-EBF273E7C035";
const SELECTION_BASE: &str = "6144EA59-3ACA-4172-9F14-A6531E94D69B";
const PERSISTENT_GUID: &str = "5A1BF548-241F-46FD-9FB5-E4B05126EB9D";
const RECIPE_GUID: &str = "5662F619-2281-4AD9-93DC-1FCFDA1062E7";
pub(crate) const POINT_GUID: &str = "69EE2FA7-BCC7-449E-9CA9-976CEFDFED44";

fn has_type(data: &SegmentTypeData, guid: &str, base: &str, versions: &[u32]) -> bool {
    data.type_guid.as_str().eq_ignore_ascii_case(guid)
        && data.base_type_guid.value().is_some_and(|value| value.as_str().eq_ignore_ascii_case(base))
        && versions.contains(&data.version)
}

pub(crate) fn is_pipe(data: &SegmentTypeData) -> bool {
    has_type(data, PIPE_GUID, PIPE_BASE, &[2])
}

pub(crate) fn is_scalar(data: &SegmentTypeData) -> bool {
    has_type(data, SCALAR_GUID, SCALAR_BASE, &[0])
}

pub(crate) fn is_point(data: &SegmentTypeData) -> bool {
    has_type(data, POINT_GUID, GROUP_GUID, &[2, 3])
}

fn is_recipe(data: &SegmentTypeData) -> bool {
    has_type(data, RECIPE_GUID, SELECTION_BASE, &[2, 3])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PipeRoleAssessment {
    Assigned,
    UnassignedIntersect,
}

pub(crate) struct PipeIndex<'a> {
    types: HashMap<(&'a str, u32), Option<&'a SegmentType>>,
    classes: HashMap<(&'a str, u32), &'a SegmentType>,
    headers: HashMap<(&'a str, u32), Option<&'a DesignRecordHeader>>,
    owners: HashMap<(&'a str, u32), Option<&'a DesignParameterOwner>>,
    groups: HashMap<(&'a str, u32), Vec<&'a DesignConstructionOperandGroup>>,
}

fn insert_unique<'a, T>(
    ctx: &DecodeContext<'_>,
    map: &mut HashMap<(&'a str, u32), Option<&'a T>>,
    key: (&'a str, u32),
    value: &'a T,
) -> Result<(), CodecError> {
    ctx.reserve_map(map, 1, "index F3D Pipe native record")?;
    match map.entry(key) {
        std::collections::hash_map::Entry::Vacant(entry) => { entry.insert(Some(value)); }
        std::collections::hash_map::Entry::Occupied(mut entry) => { entry.insert(None); }
    }
    Ok(())
}

impl<'a> PipeIndex<'a> {
    pub(crate) fn new(ctx: &DecodeContext<'_>, native: &'a F3dNative) -> Result<Self, CodecError> {
        let mut index = Self {
            types: HashMap::new(), classes: HashMap::new(), headers: HashMap::new(),
            owners: HashMap::new(), groups: HashMap::new(),
        };
        let mut types = ctx.collect_vec(native.design_types.iter(), "collect F3D Pipe native types")?;
        ctx.stable_sort_by(&mut types, |a, b| {
            (design_segment(a.id()), a.byte_offset).cmp(&(design_segment(b.id()), b.byte_offset))
        }, |_| 0, "sort F3D Pipe native types")?;
        let mut previous = None;
        let mut class = 256_u32;
        for data in types {
            let Some(segment) = design_segment(data.id()) else { continue };
            if previous != Some(segment) { class = 256; previous = Some(segment); }
            ctx.reserve_map(&mut index.classes, 1, "index F3D Pipe dynamic class")?;
            index.classes.insert((segment, class), data);
            class = class.checked_add(1).ok_or_else(|| ctx.refuse_codec_limit("F3D Pipe dynamic class", 0, 1))?;
            for entity in data.entities.values() {
                if let Ok(entity) = u32::try_from(*entity) {
                    insert_unique(ctx, &mut index.types, (segment, entity), data)?;
                }
            }
        }
        for header in &native.design_record_headers {
            if let Some(segment) = design_segment(&header.id) {
                insert_unique(ctx, &mut index.headers, (segment, header.record_index), header)?;
            }
        }
        for owner in &native.design_parameter_owners {
            if let Some(segment) = design_segment(owner.id()) {
                insert_unique(ctx, &mut index.owners, (segment, owner.record_index()), owner)?;
            }
        }
        for group in &native.design_construction_operand_groups {
            if let Some(segment) = design_segment(&group.id) {
                ctx.push_hash_group(&mut index.groups, (segment, group.scope_record_index), group,
                    "index F3D Pipe group owner", "collect F3D Pipe owned groups")?;
            }
        }
        Ok(index)
    }

    pub(crate) fn owner(&self, segment: &str, id: u32) -> Option<&DesignParameterOwner> {
        self.owners.get(&(segment, id)).and_then(|owner| *owner)
    }

    pub(crate) fn groups(&self, segment: &str, scope: u32) -> impl Iterator<Item = &DesignConstructionOperandGroup> {
        self.groups.get(&(segment, scope)).into_iter().flatten().copied()
    }

    fn typed(&self, segment: &str, id: u32) -> Option<(&SegmentTypeData, &DesignRecordHeader)> {
        let data = self.types.get(&(segment, id))?.as_ref().copied()?;
        let header = self.headers.get(&(segment, id))?.as_ref().copied()?;
        let class = header.class_tag.as_str().parse::<u32>().ok()?;
        let selected = self.classes.get(&(segment, class))?;
        std::ptr::eq(data, *selected).then_some((data, header))
    }

    pub(crate) fn check_relation(
        &self, ctx: &DecodeContext<'_>, scope: &DesignParameterScope, pipe: &DesignPipeConstruction,
    ) -> Result<Option<PipeRoleAssessment>, CodecError> {
        let mut scratch = ctx.reserve_scoped(0, "F3D Pipe relation scratch")?;
        scratch.with_storage(|| self.check(ctx, scope, pipe))
    }

    fn check(&self, ctx: &DecodeContext<'_>, scope: &DesignParameterScope, pipe: &DesignPipeConstruction)
        -> Result<Option<PipeRoleAssessment>, CodecError> {
        let Some(segment) = design_segment(&scope.id) else { return Ok(None) };
        let Some((data, header)) = self.typed(segment, scope.record_index) else { return Ok(None) };
        if !is_pipe(data) || header.byte_offset != scope.byte_offset() || header.class_tag != scope.class_tag
            || !pipe.scope_offsets_are_valid(scope.byte_offset(), scope.reference_count_offset()) {
            return Ok(None);
        }
        let mut references = HashSet::new();
        for id in scope.reference_members().values() {
            ctx.charge_work(1, "check F3D Pipe reference")?;
            if !ctx.insert_hash_set(&mut references, *id, "f3d Pipe reference record")? {
                return Ok(None);
            }
        }
        let mut claimed = HashSet::new();
        let mut previous = None;
        for (ordinal, id) in pipe.record_indexes.iter().copied().enumerate() {
            let Some((data, header)) = self.typed(segment, id) else { return Ok(None) };
            let Some(owner) = self.owner(segment, id) else { return Ok(None) };
            let position = scope.reference_members().values().position(|value| *value == id);
            if !is_scalar(data) || owner.scope_record_index() != scope.record_index
                || owner.local_ordinal() != u32::try_from(ordinal).map_err(|_| ctx.refuse_codec_limit("F3D Pipe scalar ordinal", 0, 1))?
                || owner.byte_offset() != header.byte_offset || owner.class_tag() != &header.class_tag
                || !matches!(owner.frame_length(), 103 | 104)
                || owner.byte_offset().checked_add(40) != Some(owner.evaluated_value_offset())
                || owner.byte_offset().checked_add(owner.frame_length()).is_none()
                || position.is_none() || previous.is_some_and(|previous| position <= Some(previous))
                || !ctx.insert_hash_set(&mut claimed, id, "f3d Pipe claimed record")? {
                return Ok(None);
            }
            previous = position;
        }
        let mut settings = 0_u32;
        for id in scope.reference_members().values().copied() {
            let Some((data, _)) = self.typed(segment, id) else { return Ok(None) };
            if has_type(data, SETTINGS_GUID, SETTINGS_BASE, &[1]) {
                settings = settings.checked_add(1).ok_or_else(|| ctx.refuse_codec_limit("F3D Pipe settings count", 0, 1))?;
                if !ctx.insert_hash_set(&mut claimed, id, "f3d Pipe claimed record")? { return Ok(None) }
            }
        }
        if settings != 1 { return Ok(None) }
        let mut groups = ctx.collect_vec(
            self.groups.get(&(segment, scope.record_index)).into_iter().flatten().copied(),
            "collect F3D Pipe relation groups")?;
        ctx.stable_sort_by(&mut groups, |a, b| a.scope_reference_ordinal.cmp(&b.scope_reference_ordinal), |_| 0,
            "sort F3D Pipe relation groups")?;
        for group in &groups {
            let Some((data, header)) = self.typed(segment, group.record_index) else { return Ok(None) };
            let ordinal = usize::try_from(group.scope_reference_ordinal).map_err(|_| ctx.refuse_codec_limit("F3D Pipe group ordinal", 0, 1))?;
            if !has_type(data, GROUP_GUID, GROUP_BASE, &[1]) || group.byte_offset != header.byte_offset
                || group.class_tag != header.class_tag || group.members().is_empty() || group.extrude_role().is_some()
                || group.paired_byte_offset <= group.role_offset()
                || ![47, 48, 51, 52].iter().any(|length| group.frame.opaque_index_offset().checked_add(*length) == Some(group.paired_byte_offset))
                || group.byte_offset.checked_add(21).is_none_or(|offset| group.frame.member_count_offset < offset)
                || scope.reference_members().values().nth(ordinal) != Some(&group.record_index)
                || !ctx.insert_hash_set(&mut claimed, group.record_index, "f3d Pipe claimed record")? {
                return Ok(None);
            }
            let member_start = ordinal.checked_add(1).ok_or_else(|| ctx.refuse_codec_limit("F3D Pipe member run", 0, 1))?;
            let mut offset = group.frame.member_count_offset.checked_add(5);
            for (member, scope_member) in group.members().iter().zip(scope.reference_members().values().skip(member_start)) {
                let Some((data, _)) = self.typed(segment, member.value) else { return Ok(None) };
                let role = group.role().raw();
                let typed = match role {
                    0x0000_0004_0000_0000 | 0x0000_0008_0000_0000 => has_type(data, BODY_GUID, SELECTION_BASE, &[1]),
                    0x0000_0005_0000_0000 => has_type(data, PERSISTENT_GUID, SELECTION_BASE, &[1]) || is_recipe(data),
                    _ => false,
                };
                if !typed || member.value != *scope_member || offset != Some(member.offset)
                    || member.offset.checked_add(10).is_none_or(|end| end > group.role_offset())
                    || !ctx.insert_hash_set(&mut claimed, member.value, "f3d Pipe claimed record")? { return Ok(None) }
                offset = member.offset.checked_add(11);
            }
            if scope.reference_members().len().checked_sub(member_start).is_none_or(|count| count < group.members().len()) {
                return Ok(None);
            }
        }
        if let Some(selection) = &pipe.point_selection {
            let point = &selection.point_data;
            let Some((data, header)) = self.typed(segment, point.point_record_index) else { return Ok(None) };
            let [input] = point.rule.inputs() else { return Ok(None) };
            let Some((input_type, _)) = self.typed(segment, input.record_index()) else { return Ok(None) };
            if !is_point(data) || !is_recipe(input_type) || point.rule.reference_type() != 1
                || point.point_record_byte_offset != header.byte_offset
                || point.point_record_byte_offset.checked_add(42).is_none_or(|offset| point.position_offset < offset)
                || point.position_offset.checked_add(24) != Some(point.reference_type_offset)
                || point.reference_type_offset.checked_add(if data.version == 3 { 33 } else { 9 }) != Some(input.reference_offset)
                || !ctx.insert_hash_set(&mut claimed, point.point_record_index, "f3d Pipe claimed record")?
                || !ctx.insert_hash_set(&mut claimed, input.record_index(), "f3d Pipe claimed record")? { return Ok(None) }
        }
        if claimed != references { return Ok(None) }
        let valid = match pipe.operation {
            DesignExtrudeOperation::NewBody => groups.len() == 1 && groups[0].role().raw() == 0x0000_0005_0000_0000,
            DesignExtrudeOperation::Join | DesignExtrudeOperation::Cut => groups.len() == 2
                && matches!(groups[0].role().raw(), 0x0000_0004_0000_0000 | 0x0000_0008_0000_0000)
                && groups[1].role().raw() == 0x0000_0005_0000_0000,
            DesignExtrudeOperation::Intersect => return Ok(Some(PipeRoleAssessment::UnassignedIntersect)),
        };
        Ok(valid.then_some(PipeRoleAssessment::Assigned))
    }
}

pub(crate) fn admit_native(ctx: &DecodeContext<'_>, native: &F3dNative) -> Result<(), cadmpeg_ir::NativeConvertError> {
    if !native.design_parameter_scopes.iter().any(|scope| matches!(scope.payload(), DesignScopePayload::Pipe(Some(_)))) {
        return Ok(());
    }
    let mut scratch = ctx.reserve_scoped(0, "F3D Pipe native index scratch")?;
    let valid = scratch.with_storage(|| {
        let index = PipeIndex::new(ctx, native)?;
        for scope in &native.design_parameter_scopes {
            if let DesignScopePayload::Pipe(Some(pipe)) = scope.payload() {
                if index.check_relation(ctx, scope, pipe)?.is_none() {
                    return Ok::<_, CodecError>(false);
                }
            }
        }
        Ok(true)
    })?;
    drop(scratch);
    if !valid {
        return Err(cadmpeg_ir::NativeConvertError::InvalidCollection(ctx.copy_retained_text(
            "invalid F3D Pipe native relation", "retain F3D Pipe native admission error")?));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
