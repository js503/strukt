use thiserror::Error;

const HEADER_SEPARATOR: &[u8] = b"\r\n\r\n";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameLimits {
    max_header_bytes: usize,
    max_body_bytes: usize,
}

impl FrameLimits {
    /// Creates non-zero frame limits.
    ///
    /// # Errors
    ///
    /// Returns [`FrameError::InvalidLimits`] when either limit is zero.
    pub fn new(max_header_bytes: usize, max_body_bytes: usize) -> Result<Self, FrameError> {
        if max_header_bytes == 0 || max_body_bytes == 0 {
            return Err(FrameError::InvalidLimits);
        }
        Ok(Self {
            max_header_bytes,
            max_body_bytes,
        })
    }
}

impl Default for FrameLimits {
    fn default() -> Self {
        Self {
            max_header_bytes: 16 * 1024,
            max_body_bytes: 16 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    body: Vec<u8>,
}

impl Frame {
    #[must_use]
    pub fn body(&self) -> &[u8] {
        &self.body
    }
}

#[derive(Debug)]
pub struct FrameDecoder {
    limits: FrameLimits,
    buffer: Vec<u8>,
}

impl FrameDecoder {
    #[must_use]
    pub const fn new(limits: FrameLimits) -> Self {
        Self {
            limits,
            buffer: Vec::new(),
        }
    }

    #[must_use]
    pub fn buffered_bytes(&self) -> usize {
        self.buffer.len()
    }

    /// Adds bytes and returns every complete frame now available.
    ///
    /// # Errors
    ///
    /// Returns a framing error for malformed or oversized input. The internal
    /// buffer is cleared before an error is returned.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Frame>, FrameError> {
        self.buffer.extend_from_slice(bytes);
        match self.decode_available() {
            Ok(frames) => Ok(frames),
            Err(error) => {
                self.buffer.clear();
                Err(error)
            }
        }
    }

    fn decode_available(&mut self) -> Result<Vec<Frame>, FrameError> {
        let mut frames = Vec::new();
        loop {
            let Some(header_end) = find_subslice(&self.buffer, HEADER_SEPARATOR) else {
                if self.buffer.len() > self.limits.max_header_bytes {
                    return Err(FrameError::HeaderTooLarge);
                }
                break;
            };
            if header_end > self.limits.max_header_bytes {
                return Err(FrameError::HeaderTooLarge);
            }

            let body_length = parse_content_length(&self.buffer[..header_end])?;
            if body_length > self.limits.max_body_bytes {
                return Err(FrameError::BodyTooLarge {
                    declared: body_length,
                });
            }

            let body_start = header_end + HEADER_SEPARATOR.len();
            let frame_end =
                body_start
                    .checked_add(body_length)
                    .ok_or(FrameError::BodyTooLarge {
                        declared: body_length,
                    })?;
            if self.buffer.len() < frame_end {
                break;
            }

            let body = self.buffer[body_start..frame_end].to_vec();
            self.buffer.drain(..frame_end);
            frames.push(Frame { body });
        }
        Ok(frames)
    }
}

/// Encodes one Language Server Protocol frame.
///
/// # Errors
///
/// Returns [`FrameError::BodyTooLarge`] if `body` exceeds the configured limit.
pub fn encode_frame(body: &[u8], limits: FrameLimits) -> Result<Vec<u8>, FrameError> {
    if body.len() > limits.max_body_bytes {
        return Err(FrameError::BodyTooLarge {
            declared: body.len(),
        });
    }
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    if header.len() - HEADER_SEPARATOR.len() > limits.max_header_bytes {
        return Err(FrameError::HeaderTooLarge);
    }
    let mut encoded = Vec::with_capacity(header.len() + body.len());
    encoded.extend_from_slice(header.as_bytes());
    encoded.extend_from_slice(body);
    Ok(encoded)
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn parse_content_length(headers: &[u8]) -> Result<usize, FrameError> {
    let text = std::str::from_utf8(headers).map_err(|_| FrameError::MalformedHeader)?;
    let mut content_length = None;
    for line in text.split("\r\n") {
        let (name, value) = line.split_once(':').ok_or(FrameError::MalformedHeader)?;
        if name.trim().eq_ignore_ascii_case("content-length") {
            if content_length.is_some() {
                return Err(FrameError::MalformedHeader);
            }
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| FrameError::MalformedHeader)?,
            );
        }
    }
    content_length.ok_or(FrameError::MissingContentLength)
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum FrameError {
    #[error("frame limits must be non-zero")]
    InvalidLimits,
    #[error("frame header exceeds its byte limit")]
    HeaderTooLarge,
    #[error("frame body exceeds its byte limit: {declared} bytes declared")]
    BodyTooLarge { declared: usize },
    #[error("frame header is malformed")]
    MalformedHeader,
    #[error("frame has no Content-Length header")]
    MissingContentLength,
}
