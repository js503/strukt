use strukt_language::{FrameDecoder, FrameError, FrameLimits, encode_frame};

#[test]
fn decoder_handles_fragmented_and_combined_frames() {
    let mut decoder = FrameDecoder::new(FrameLimits::default());
    assert!(decoder.push(b"Content-Len").unwrap().is_empty());
    let frames = decoder
        .push(b"gth: 2\r\n\r\n{}Content-Length: 4\r\n\r\nnull")
        .unwrap();

    assert_eq!(
        frames
            .iter()
            .map(strukt_language::Frame::body)
            .collect::<Vec<_>>(),
        vec![b"{}".as_slice(), b"null".as_slice()]
    );
}

#[test]
fn decoder_accepts_case_insensitive_content_length_and_unknown_headers() {
    let mut decoder = FrameDecoder::new(FrameLimits::default());
    let frames = decoder
        .push(b"content-length: 2\r\nX-Trace: ignored\r\n\r\n{}")
        .unwrap();

    assert_eq!(frames[0].body(), b"{}");
}

#[test]
fn decoder_rejects_oversized_headers_and_bodies_without_retaining_them() {
    let limits = FrameLimits::new(32, 64).unwrap();
    let mut headers = FrameDecoder::new(limits);
    assert_eq!(headers.push(&[b'x'; 33]), Err(FrameError::HeaderTooLarge));
    assert_eq!(headers.buffered_bytes(), 0);

    let mut bodies = FrameDecoder::new(limits);
    assert_eq!(
        bodies.push(b"Content-Length: 65\r\n\r\n"),
        Err(FrameError::BodyTooLarge { declared: 65 })
    );
    assert_eq!(bodies.buffered_bytes(), 0);
}

#[test]
fn encoder_uses_the_exact_utf8_body_length() {
    let encoded = encode_frame("λ".as_bytes(), FrameLimits::default()).unwrap();
    assert_eq!(encoded, b"Content-Length: 2\r\n\r\n\xce\xbb");
}
