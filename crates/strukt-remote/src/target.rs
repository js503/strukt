use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

const MAX_ALIAS_BYTES: usize = 255;
const MAX_REMOTE_ROOT_BYTES: usize = 4_096;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum RemoteTargetError {
    #[error("the operating-system random source is unavailable")]
    RandomUnavailable,
    #[error("the connection identifier must be exactly 32 lowercase hexadecimal characters")]
    InvalidConnectionId,
    #[error("the SSH alias is invalid")]
    InvalidAlias,
    #[error("the remote root must be an absolute or home-rooted normalized Linux path")]
    InvalidRemoteRoot,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ConnectionId([u8; 16]);

impl ConnectionId {
    /// Generates an opaque connection identifier from the operating-system random
    /// source.
    ///
    /// # Errors
    ///
    /// Returns [`RemoteTargetError::RandomUnavailable`] when the random source
    /// cannot fill the identifier.
    pub fn new() -> Result<Self, RemoteTargetError> {
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| RemoteTargetError::RandomUnavailable)?;
        Ok(Self(bytes))
    }

    #[must_use]
    pub const fn as_bytes(self) -> [u8; 16] {
        self.0
    }
}

impl fmt::Display for ConnectionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_lower_hex(formatter, &self.0)
    }
}

impl FromStr for ConnectionId {
    type Err = RemoteTargetError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != 32 || !value.bytes().all(is_lower_hex) {
            return Err(RemoteTargetError::InvalidConnectionId);
        }
        let mut bytes = [0_u8; 16];
        for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
            bytes[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
        }
        Ok(Self(bytes))
    }
}

impl Serialize for ConnectionId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ConnectionId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_str(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SshAlias(String);

impl SshAlias {
    /// Validates an opaque OpenSSH host alias.
    ///
    /// # Errors
    ///
    /// Returns [`RemoteTargetError::InvalidAlias`] for an empty, option-like,
    /// control-containing, whitespace-containing, or oversized value.
    pub fn new(value: impl Into<String>) -> Result<Self, RemoteTargetError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_ALIAS_BYTES
            || value.starts_with('-')
            || value.chars().any(char::is_whitespace)
            || value.chars().any(char::is_control)
        {
            return Err(RemoteTargetError::InvalidAlias);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SshAlias {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SshAlias {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct RemoteRoot(String);

impl RemoteRoot {
    /// Validates and lexically normalizes a Linux remote workspace root.
    ///
    /// # Errors
    ///
    /// Returns [`RemoteTargetError::InvalidRemoteRoot`] when the value is not
    /// absolute or home-rooted, contains an escape/control value, or exceeds the
    /// protocol path bound.
    pub fn new(value: impl AsRef<str>) -> Result<Self, RemoteTargetError> {
        let value = value.as_ref();
        if value.is_empty()
            || value.len() > MAX_REMOTE_ROOT_BYTES
            || value.chars().any(char::is_control)
        {
            return Err(RemoteTargetError::InvalidRemoteRoot);
        }
        let (prefix, rest) = if let Some(rest) = value.strip_prefix("~/") {
            ("~", rest)
        } else if value == "~" {
            ("~", "")
        } else if let Some(rest) = value.strip_prefix('/') {
            ("", rest)
        } else {
            return Err(RemoteTargetError::InvalidRemoteRoot);
        };

        let mut segments = Vec::new();
        for segment in rest.split('/') {
            match segment {
                "" | "." => {}
                ".." => return Err(RemoteTargetError::InvalidRemoteRoot),
                _ => segments.push(segment),
            }
        }

        let normalized = if prefix == "~" {
            if segments.is_empty() {
                "~".to_owned()
            } else {
                format!("~/{}", segments.join("/"))
            }
        } else if segments.is_empty() {
            "/".to_owned()
        } else {
            format!("/{}", segments.join("/"))
        };
        Ok(Self(normalized))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RemoteRoot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RemoteRoot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RemoteWorkspaceId([u8; 32]);

impl RemoteWorkspaceId {
    #[must_use]
    pub fn derive(connection: ConnectionId, root: &RemoteRoot) -> Self {
        let mut hasher = blake3::Hasher::new_derive_key("strukt remote workspace identity v1");
        hasher.update(&connection.as_bytes());
        hasher.update(&[0]);
        hasher.update(root.as_str().as_bytes());
        Self(*hasher.finalize().as_bytes())
    }
}

impl fmt::Display for RemoteWorkspaceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_lower_hex(formatter, &self.0)
    }
}

fn write_lower_hex(formatter: &mut fmt::Formatter<'_>, bytes: &[u8]) -> fmt::Result {
    for byte in bytes {
        write!(formatter, "{byte:02x}")?;
    }
    Ok(())
}

const fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (byte >= b'a' && byte <= b'f')
}

const fn hex_nibble(byte: u8) -> Result<u8, RemoteTargetError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(RemoteTargetError::InvalidConnectionId),
    }
}
