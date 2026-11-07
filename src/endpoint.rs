//! Typed radio routing and repository identity, stored after authenticated enrollment.
use crate::model::ShareId;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Address(pub [u8; 6]);
impl FromStr for Address {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self> {
        let bytes = value.as_bytes();
        ensure!(
            bytes.len() == 17,
            "Bluetooth address must contain six hex byte pairs"
        );
        let separator = bytes[2];
        ensure!(
            matches!(separator, b':' | b'-'),
            "invalid Bluetooth address separator"
        );
        let mut address = [0; 6];
        for (i, byte) in address.iter_mut().enumerate() {
            let start = i * 3;
            ensure!(
                bytes[start..start + 2].iter().all(u8::is_ascii_hexdigit),
                "invalid Bluetooth address"
            );
            if i < 5 {
                ensure!(
                    bytes[start + 2] == separator,
                    "mixed Bluetooth address separators"
                );
            }
            *byte = u8::from_str_radix(&value[start..start + 2], 16)?;
        }
        ensure!(address != [0; 6], "invalid all-zero Bluetooth address");
        Ok(Self(address))
    }
}
impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, byte) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(":")?;
            }
            write!(f, "{byte:02X}")?;
        }
        Ok(())
    }
}
impl Address {
    pub fn as_u64(self) -> u64 {
        self.0.into_iter().fold(0, |n, b| (n << 8) | u64::from(b))
    }
    pub fn from_u64(value: u64) -> Result<Self> {
        ensure!(
            value > 0 && value <= 0xffffffffffff,
            "invalid Bluetooth address integer"
        );
        let raw = value.to_be_bytes();
        Ok(Self(raw[2..].try_into()?))
    }
}
impl TryFrom<String> for Address {
    type Error = anyhow::Error;
    fn try_from(v: String) -> Result<Self> {
        v.parse()
    }
}
impl From<Address> for String {
    fn from(a: Address) -> Self {
        a.to_string()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct Channel(u8);
impl Channel {
    pub fn get(self) -> u8 {
        self.0
    }
}
impl TryFrom<u8> for Channel {
    type Error = anyhow::Error;
    fn try_from(v: u8) -> Result<Self> {
        ensure!((1..=30).contains(&v), "RFCOMM channel must be 1..30");
        Ok(Self(v))
    }
}
impl From<Channel> for u8 {
    fn from(c: Channel) -> u8 {
        c.0
    }
}
impl FromStr for Channel {
    type Err = anyhow::Error;
    fn from_str(v: &str) -> Result<Self> {
        ensure!(
            !v.is_empty() && v.len() <= 2 && v.bytes().all(|b| b.is_ascii_digit()),
            "invalid RFCOMM channel"
        );
        Self::try_from(v.parse::<u8>()?)
    }
}
impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub address: Address,
    pub share: ShareId,
    pub channel: Channel,
}
/// Radio routing information available before repository enrollment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Contact {
    pub address: Address,
    pub channel: Channel,
}
impl Endpoint {
    pub fn contact(&self) -> Contact {
        Contact {
            address: self.address,
            channel: self.channel,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persisted_routes_reject_invalid_addresses_and_channels() {
        for address in [
            "",
            "00:00:00:00:00:00",
            "AA-BB:CC-DD-EE-FF",
            "ああ:BB:CC:DD:EE:FF",
            "AA:BB:CC:DD:EE:GG",
        ] {
            assert!(address.parse::<Address>().is_err());
        }
        for channel in ["0", "31", "7&channel=8", "-1", ""] {
            assert!(channel.parse::<Channel>().is_err());
        }
    }
}
