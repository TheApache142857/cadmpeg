// SPDX-License-Identifier: Apache-2.0

use crate::loss::CreoLossCode;
use crate::CreoCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::EditableDecodeResult;
use std::io::Cursor;

#[test]
fn compressed_section_recovery_reports_loss_and_retains_the_bounded_source() {
    for name in [
        "SolidPrimdata",
        "DispDataTable",
        "THMB_IMG_MAIN",
        "VisibGeom",
    ] {
        let mut bytes = b"#UGC:2 P test\n#-END_OF_UGC_HEADER\n".to_vec();
        let header_base = bytes.len();
        bytes.extend_from_slice(format!("{:<80}\n", "#UGC_TOC 2 2 81 17").as_bytes());
        let mut damaged = format!("#{name}\n").into_bytes();
        damaged.extend_from_slice(&[0x1f, 0x9d, 0x10, 0x41, 0x58, 0x02]);
        // A rejected compressed body must not become a raw PSB input.
        damaged.extend_from_slice(b"srf_array\0\xf8\x05");
        let mut readable = b"#SolidPersistTable\n".to_vec();
        readable.extend_from_slice(&[0x1f, 0x9d, 0x10, 0x41, 0x84, 0x0c, 0x01]);
        let first_offset = 3 * 81;
        let second_offset = first_offset + damaged.len();
        for row in [
            format!("{name} {first_offset:x} {:x} 5", damaged.len()),
            format!("SolidPersistTable {second_offset:x} {:x} 3", readable.len()),
        ] {
            bytes.extend_from_slice(format!("{row:<80}\n").as_bytes());
        }
        assert_eq!(bytes.len(), header_base + first_offset);
        bytes.extend_from_slice(&damaged);
        bytes.extend(readable);
        let decoded = CreoCodec
            .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
            .expect("independent section is decoded");
        let decoded = EditableDecodeResult::from(decoded);
        let losses = decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.to_string() == "creo/container.compressed-section-unexpanded")
            .collect::<Vec<_>>();
        assert_eq!(losses.len(), 1);
        assert!(losses[0].message.contains("undefined code"));
        assert!(decoded
            .source_fidelity()
            .retained_records()
            .values()
            .any(|record| record.offset()
                == cadmpeg_core::decode::u64_from_index(header_base + first_offset)
                && record.data() == Some(damaged.as_slice())));
        let scan = crate::container::scan_bytes_ok(bytes);
        assert_eq!(scan.framing.sections.len(), 2);
        assert_eq!(scan.framing.expanded_sections.len(), 1);
        assert_eq!(scan.framing.expanded_sections[0].data, b"ABC");
        assert_eq!(scan.framing.census.srf_array_count, None);
    }
}

#[test]
fn enciphered_storage_codes_retain_the_stored_bytes_without_reading_them() {
    let mut bytes = b"#UGC:2 P test\n#-END_OF_UGC_HEADER\n".to_vec();
    let header_base = bytes.len();
    bytes.extend_from_slice(format!("{:<80}\n", "#UGC_TOC 2 3 81 17").as_bytes());
    // Under code `a` this stream expands to `ABC`; code `c` enciphers it.
    let mut enciphered_stream = b"#DispDataTable\n".to_vec();
    enciphered_stream.extend_from_slice(&[0x1f, 0x9d, 0x10, 0x41, 0x84, 0x0c, 0x01]);
    let mut enciphered_records = b"#VisibGeom\n\xe0\x00Fi".to_vec();
    enciphered_records.extend_from_slice(b"srf_array\0\xf8\x05");
    let mut readable = b"#SolidPersistTable\n".to_vec();
    readable.extend_from_slice(&[0x1f, 0x9d, 0x10, 0x41, 0x84, 0x0c, 0x01]);
    let first_offset = 4 * 81;
    let second_offset = first_offset + enciphered_stream.len();
    let third_offset = second_offset + enciphered_records.len();
    for row in [
        format!(
            "DispDataTable {first_offset:x} {:x} 3 2562 c 1 0000 0000",
            enciphered_stream.len()
        ),
        format!(
            "VisibGeom {second_offset:x} {:x} {:x} 2562 b -1 0000 0000",
            enciphered_records.len(),
            enciphered_records.len() - b"#VisibGeom\n".len()
        ),
        format!(
            "SolidPersistTable {third_offset:x} {:x} 3 2562 a 1 0000 0000",
            readable.len()
        ),
    ] {
        bytes.extend_from_slice(format!("{row:<80}\n").as_bytes());
    }
    assert_eq!(bytes.len(), header_base + first_offset);
    bytes.extend_from_slice(&enciphered_stream);
    bytes.extend_from_slice(&enciphered_records);
    bytes.extend_from_slice(&readable);

    let decoded = CreoCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("enciphered sections are retained");
    let decoded = EditableDecodeResult::from(decoded);
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.to_string() == "creo/container.compressed-section-unexpanded"));
    let enciphered = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == CreoLossCode::EncipheredSectionRetained.kind())
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    assert_eq!(enciphered.len(), 2);
    assert!(enciphered[0].contains("`DispDataTable`") && enciphered[0].contains("code `c`"));
    assert!(enciphered[1].contains("`VisibGeom`") && enciphered[1].contains("code `b`"));
    for (offset, section) in [
        (first_offset, &enciphered_stream),
        (second_offset, &enciphered_records),
    ] {
        assert!(decoded
            .source_fidelity()
            .retained_records()
            .values()
            .any(|record| record.offset()
                == cadmpeg_core::decode::u64_from_index(header_base + offset)
                && record.data() == Some(section.as_slice())));
    }
    let scan = crate::container::scan_bytes_ok(bytes);
    assert_eq!(scan.framing.expanded_sections.len(), 1);
    assert_eq!(scan.framing.expanded_sections[0].name, "SolidPersistTable");
    assert_eq!(scan.framing.census.srf_array_count, None);
}
