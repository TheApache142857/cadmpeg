// SPDX-License-Identifier: Apache-2.0

use std::fmt::Display;

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::StreamHandle;
use cadmpeg_ir::AnnotationBuilder;

use crate::parasolid::Stream;
use crate::raw_origin::OriginMap;

pub(super) struct RawGeometryStream {
    pub(super) handle: StreamHandle,
    pub(super) len: usize,
}

impl RawGeometryStream {
    pub(super) fn new(
        ctx: &DecodeContext<'_>,
        ordinal: usize,
        stream: &Stream,
    ) -> Result<Self, CodecError> {
        let name = ctx.format_retained(
            format_args!("nx:parasolid#{ordinal}:{}", stream.kind().label()),
            "nx geometry stream name",
        )?;
        let handle = StreamHandle::new(
            ctx,
            cadmpeg_ir::StreamName::try_from(name).map_err(CodecError::malformed)?,
            "allocate annotation stream handle",
        )?;
        Ok(Self {
            handle,
            len: stream.inflated.len(),
        })
    }
}

#[derive(Clone, Copy)]
pub(super) struct GeometryOrigins<'a> {
    pub(super) map: &'a OriginMap,
    pub(super) streams: &'a [RawGeometryStream],
}

impl GeometryOrigins<'_> {
    pub(super) fn note_record(
        self,
        ctx: &DecodeContext<'_>,
        annotations: &mut AnnotationBuilder,
        id: impl Display,
        pos: usize,
        tag: &str,
    ) -> Result<(), CodecError> {
        let origin = self.map.record_start(ctx, pos)?;
        let stream = self
            .streams
            .get(origin.stream.index())
            .filter(|stream| origin.offset < stream.len)
            .ok_or_else(|| CodecError::malformed("NX geometry origin is outside its raw stream"))?;
        annotations.note(
            ctx,
            id,
            &stream.handle,
            u64_from_index(origin.offset),
            Some(tag),
        )
    }
}
