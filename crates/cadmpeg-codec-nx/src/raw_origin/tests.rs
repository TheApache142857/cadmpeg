// SPDX-License-Identifier: Apache-2.0

use super::{OriginMap, RawRecordOrigin, RawSource, RawStreamOrdinal};

#[test]
fn raw_sources_map_every_byte_to_its_stream_offset_and_refuse_the_end() {
    crate::test_support::with_decode_context(|ctx| {
        let bytes = [0; 12];
        let source = RawSource::raw(RawStreamOrdinal::new(3), &bytes);
        assert!(std::ptr::eq(source.bytes(), bytes.as_slice()));
        for offset in [0, 5, 11] {
            assert_eq!(
                source.origins().record_start(ctx, offset)?,
                RawRecordOrigin {
                    stream: RawStreamOrdinal::new(3),
                    offset,
                }
            );
        }
        assert!(matches!(
            source.origins().record_start(ctx, 12),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
        assert!(matches!(
            source.origins().record_start(ctx, usize::MAX),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .unwrap();
}

#[test]
fn empty_sources_have_no_origin() {
    crate::test_support::with_decode_context(|ctx| {
        let source = RawSource::default();
        assert!(source.bytes().is_empty());
        assert!(matches!(
            source.origins().record_start(ctx, 0),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
        assert!(matches!(
            OriginMap::default().record_start(ctx, 0),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
        assert!(matches!(
            OriginMap::raw(RawStreamOrdinal::new(1), 0).record_start(ctx, 0),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    });
}

#[test]
fn raw_origin_lookups_refuse_at_the_work_limit() {
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_work_units = 0;
        },
        |ctx| {
            let error = OriginMap::raw(RawStreamOrdinal::new(0), 4)
                .record_start(ctx, 0)
                .expect_err("one origin lookup exceeds zero work units");
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.operation == "NX raw record origin lookup"
            ));
        },
    );
}
