use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum IdError {
    #[error("the operating system random source is unavailable")]
    RandomUnavailable,
    #[error("the identifier must be exactly 32 lowercase hexadecimal characters")]
    InvalidFormat,
}

macro_rules! opaque_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name([u8; 16]);

        impl $name {
            /// Generates a new identifier from the operating-system random source.
            ///
            /// # Errors
            ///
            /// Returns [`IdError::RandomUnavailable`] when the OS source fails.
            pub fn new() -> Result<Self, IdError> {
                let mut bytes = [0_u8; 16];
                getrandom::fill(&mut bytes).map_err(|_| IdError::RandomUnavailable)?;
                Ok(Self(bytes))
            }

            #[must_use]
            pub const fn as_bytes(self) -> [u8; 16] {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                for byte in self.0 {
                    write!(formatter, "{byte:02x}")?;
                }
                Ok(())
            }
        }

        impl FromStr for $name {
            type Err = IdError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                if value.len() != 32
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                {
                    return Err(IdError::InvalidFormat);
                }
                let mut bytes = [0_u8; 16];
                for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
                    bytes[index] = (nibble(pair[0])? << 4) | nibble(pair[1])?;
                }
                Ok(Self(bytes))
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::from_str(&value).map_err(serde::de::Error::custom)
            }
        }
    };
}

fn nibble(byte: u8) -> Result<u8, IdError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(IdError::InvalidFormat),
    }
}

opaque_id!(SessionId);
opaque_id!(WindowId);
opaque_id!(PaneId);
opaque_id!(ServiceInstanceId);
