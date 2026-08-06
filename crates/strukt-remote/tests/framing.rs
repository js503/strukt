use std::io::{Cursor, Read, Write};

use serde::{Deserialize, Serialize};
use strukt_remote::{
    DEFAULT_FRAME_LIMIT, FramingError, read_frame, read_preface, write_frame, write_preface,
};

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Fixture {
    name: String,
    value: u64,
}

struct ShortIo {
    inner: Cursor<Vec<u8>>,
    chunk: usize,
}

impl ShortIo {
    fn new(bytes: Vec<u8>, chunk: usize) -> Self {
        Self {
            inner: Cursor::new(bytes),
            chunk,
        }
    }

    fn into_inner(self) -> Vec<u8> {
        self.inner.into_inner()
    }
}

impl Read for ShortIo {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let limit = buffer.len().min(self.chunk);
        self.inner.read(&mut buffer[..limit])
    }
}

impl Write for ShortIo {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let limit = buffer.len().min(self.chunk);
        self.inner.write(&buffer[..limit])
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn preface_and_frames_survive_partial_io_and_multiple_messages() {
    let mut writer = ShortIo::new(Vec::new(), 2);
    write_preface(&mut writer).unwrap();
    write_frame(
        &mut writer,
        &Fixture {
            name: "alpha".into(),
            value: 1,
        },
        DEFAULT_FRAME_LIMIT,
    )
    .unwrap();
    write_frame(
        &mut writer,
        &Fixture {
            name: "beta".into(),
            value: 2,
        },
        DEFAULT_FRAME_LIMIT,
    )
    .unwrap();

    let mut reader = ShortIo::new(writer.into_inner(), 1);
    read_preface(&mut reader).unwrap();
    assert_eq!(
        read_frame::<_, Fixture>(&mut reader, DEFAULT_FRAME_LIMIT).unwrap(),
        Fixture {
            name: "alpha".into(),
            value: 1
        }
    );
    assert_eq!(
        read_frame::<_, Fixture>(&mut reader, DEFAULT_FRAME_LIMIT).unwrap(),
        Fixture {
            name: "beta".into(),
            value: 2
        }
    );
}

#[test]
fn rejects_bad_preface_zero_oversized_truncated_and_invalid_cbor() {
    assert!(matches!(
        read_preface(&mut Cursor::new(b"STRUKT-REMOTE\0\x02")),
        Err(FramingError::InvalidPreface)
    ));

    for bytes in [
        0_u32.to_be_bytes().to_vec(),
        (u32::try_from(DEFAULT_FRAME_LIMIT).unwrap() + 1)
            .to_be_bytes()
            .to_vec(),
    ] {
        assert!(matches!(
            read_frame::<_, Fixture>(&mut Cursor::new(bytes), DEFAULT_FRAME_LIMIT),
            Err(FramingError::InvalidLength)
        ));
    }

    let mut truncated = 8_u32.to_be_bytes().to_vec();
    truncated.extend_from_slice(&[1, 2]);
    assert!(matches!(
        read_frame::<_, Fixture>(&mut Cursor::new(truncated), DEFAULT_FRAME_LIMIT),
        Err(FramingError::Io(_))
    ));

    let mut invalid = 1_u32.to_be_bytes().to_vec();
    invalid.push(0xff);
    assert!(matches!(
        read_frame::<_, Fixture>(&mut Cursor::new(invalid), DEFAULT_FRAME_LIMIT),
        Err(FramingError::InvalidCbor)
    ));
}

#[test]
fn rejects_trailing_cbor_and_write_overflow() {
    let mut payload = Vec::new();
    ciborium::into_writer(
        &Fixture {
            name: "alpha".into(),
            value: 1,
        },
        &mut payload,
    )
    .unwrap();
    payload.push(0);
    let mut bytes = u32::try_from(payload.len()).unwrap().to_be_bytes().to_vec();
    bytes.extend_from_slice(&payload);
    assert!(matches!(
        read_frame::<_, Fixture>(&mut Cursor::new(bytes), DEFAULT_FRAME_LIMIT),
        Err(FramingError::TrailingData)
    ));

    let fixture = Fixture {
        name: "x".repeat(1_024),
        value: 1,
    };
    assert!(matches!(
        write_frame(&mut Vec::new(), &fixture, 16),
        Err(FramingError::InvalidLength)
    ));
}

#[test]
fn end_of_stream_is_distinct_from_a_truncated_length() {
    assert!(matches!(
        read_frame::<_, Fixture>(&mut Cursor::new(Vec::<u8>::new()), DEFAULT_FRAME_LIMIT),
        Err(FramingError::EndOfStream)
    ));
    assert!(matches!(
        read_frame::<_, Fixture>(&mut Cursor::new(vec![0, 0]), DEFAULT_FRAME_LIMIT),
        Err(FramingError::Io(_))
    ));
}
