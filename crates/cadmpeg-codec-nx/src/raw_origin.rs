// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RawStreamOrdinal(usize);

impl RawStreamOrdinal {
    pub(crate) fn new(index: usize) -> Self {
        Self(index)
    }

    pub(crate) fn index(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RawRecordOrigin {
    pub(crate) stream: RawStreamOrdinal,
    pub(crate) offset: usize,
}

#[derive(Clone, Copy, Debug)]
struct RawStreamBounds {
    stream: RawStreamOrdinal,
    len: usize,
}

/// Raw-boundary proof for one borrowed stream: every position below the
/// stream length originates from that stream at the same offset.
///
/// The default map has no origin and refuses every lookup.
#[derive(Debug, Default)]
pub(crate) struct OriginMap(Option<RawStreamBounds>);

impl OriginMap {
    pub(crate) fn raw(stream: RawStreamOrdinal, len: usize) -> Self {
        Self(Some(RawStreamBounds { stream, len }))
    }

    pub(crate) fn record_start(
        &self,
        ctx: &DecodeContext<'_>,
        pos: usize,
    ) -> Result<RawRecordOrigin, CodecError> {
        ctx.charge_work(1, "NX raw record origin lookup")?;
        match self.0 {
            Some(RawStreamBounds { stream, len }) if pos < len => Ok(RawRecordOrigin {
                stream,
                offset: pos,
            }),
            _ => Err(CodecError::malformed(
                "NX geometry record has no raw source origin",
            )),
        }
    }
}

/// The original bytes of one inflated stream together with their stream origin.
///
/// The default source is empty and has no origin.
#[derive(Debug, Default)]
pub(crate) struct RawSource<'a> {
    bytes: &'a [u8],
    origins: OriginMap,
}

impl<'a> RawSource<'a> {
    pub(crate) fn raw(stream: RawStreamOrdinal, bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            origins: OriginMap::raw(stream, bytes.len()),
        }
    }

    pub(crate) fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    pub(crate) fn origins(&self) -> &OriginMap {
        &self.origins
    }
}

#[cfg(test)]
mod tests;
