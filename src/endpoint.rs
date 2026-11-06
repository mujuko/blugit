use anyhow::{Result, bail, ensure};
use std::{fmt, str::FromStr};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Address(pub [u8; 6]);

impl FromStr for Address {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        ensure!(
            value.len() == 17,
            "expected six hex byte pairs in Bluetooth address"
        );
        let bytes = value.as_bytes();
        let separator = bytes[2];
        ensure!(
            separator == b':' || separator == b'-',
            "invalid Bluetooth address separator"
        );
        let mut address = [0; 6];
        for (index, item) in address.iter_mut().enumerate() {
            let offset = index * 3;
            ensure!(
                bytes[offset..offset + 2].iter().all(u8::is_ascii_hexdigit),
                "invalid Bluetooth address"
            );
            if index < 5 {
                ensure!(
                    bytes[offset + 2] == separator,
                    "mixed Bluetooth address separators"
                );
            }
            *item = u8::from_str_radix(&value[offset..offset + 2], 16)?;
        }
        ensure!(
            address != [0; 6],
            "the adapter did not report a usable Bluetooth address"
        );
        Ok(Self(address))
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, byte) in self.0.iter().enumerate() {
            if index > 0 {
                write!(f, ":")?;
            }
            write!(f, "{byte:02X}")?;
        }
        Ok(())
    }
}

impl Address {
    pub fn url(self, channel: Option<u8>) -> String {
        let mut url = format!("blugit://{}", self.to_string().replace(':', "-"));
        if let Some(channel) = channel {
            url.push_str(&format!("?channel={channel}"));
        }
        url
    }

    pub fn from_u64(value: u64) -> Result<Self> {
        format!("{:012X}", value)
            .as_bytes()
            .chunks(2)
            .map(std::str::from_utf8)
            .collect::<std::result::Result<Vec<_>, _>>()?
            .join(":")
            .parse()
    }

    pub fn as_u64(self) -> u64 {
        self.0
            .into_iter()
            .fold(0, |result, byte| (result << 8) | u64::from(byte))
    }
}

#[derive(Clone, Debug)]
pub struct Endpoint {
    pub address: Address,
    pub channel: Option<u8>,
    pub secure: bool,
}

impl FromStr for Endpoint {
    type Err = anyhow::Error;
    fn from_str(url: &str) -> Result<Self> {
        let value = url
            .strip_prefix("blugit://")
            .ok_or_else(|| anyhow::anyhow!("expected blugit://AA-BB-CC-DD-EE-FF"))?;
        ensure!(!value.contains('#'), "URL fragments are unsupported");
        let (authority, query) = value.split_once('?').unwrap_or((value, ""));
        let address = authority.strip_suffix('/').unwrap_or(authority).parse()?;
        let mut channel = None;
        let mut secure = false;
        if !query.is_empty() {
            for parameter in query.split('&') {
                match parameter.split_once('=') {
                    Some(("channel", number)) if channel.is_none() => {
                        ensure!(
                            !number.is_empty()
                                && number.len() <= 2
                                && number.bytes().all(|b| b.is_ascii_digit()),
                            "invalid RFCOMM channel"
                        );
                        let value = number.parse()?;
                        ensure!(
                            (1..=30).contains(&value),
                            "RFCOMM channel must be between 1 and 30"
                        );
                        channel = Some(value);
                    }
                    Some(("secure", "1")) if !secure => secure = true,
                    _ => bail!("unsupported or duplicate URL parameter"),
                }
            }
        }
        Ok(Self {
            address,
            channel,
            secure,
        })
    }
}

impl Endpoint {
    pub fn secure_url(address: Address, channel: Option<u8>) -> String {
        let mut url = address.url(channel);
        url.push_str(if channel.is_some() {
            "&secure=1"
        } else {
            "?secure=1"
        });
        url
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_urls_are_compatible() -> Result<()> {
        for channel in [None, Some(1), Some(30)] {
            let address: Address = "00:a7:50:41:8d:47".parse()?;
            let parsed: Endpoint = address.url(channel).parse()?;
            assert_eq!(parsed.address, address);
            assert_eq!(parsed.channel, channel);
        }
        assert_eq!(
            "blugit://aa:bb:cc:dd:ee:ff/"
                .parse::<Endpoint>()?
                .address
                .to_string(),
            "AA:BB:CC:DD:EE:FF"
        );
        Ok(())
    }

    #[test]
    fn secure_urls_select_authentication_and_allow_a_direct_channel() -> Result<()> {
        for channel in [None, Some(1), Some(30)] {
            let address = "AA:BB:CC:DD:EE:FF".parse()?;
            let endpoint: Endpoint = Endpoint::secure_url(address, channel).parse()?;
            assert!(endpoint.secure);
            assert_eq!(endpoint.channel, channel);
            assert_eq!(endpoint.address, address);
        }
        assert!(
            "blugit://AA-BB-CC-DD-EE-FF?secure=1&channel=7"
                .parse::<Endpoint>()?
                .secure
        );
        for query in [
            "secure=0",
            "secure=1&secure=1",
            "secure=1&",
            "secure=1&unknown=1",
        ] {
            assert!(
                format!("blugit://AA-BB-CC-DD-EE-FF?{query}")
                    .parse::<Endpoint>()
                    .is_err()
            );
        }
        Ok(())
    }

    #[test]
    fn invalid_destinations_are_rejected() {
        for value in [
            "http://AA-BB-CC-DD-EE-FF",
            "blugit://hostname/repo",
            "blugit://AA-BB-CC-DD-EE-FF/../x",
            "blugit://AA-BB-CC-DD-EE-FF?channel=0",
            "blugit://AA-BB-CC-DD-EE-FF?channel=31",
            "blugit://AA-BB-CC-DD-EE-FF?channel=1&channel=2",
            "blugit://AA-BB-CC-DD-EE-FF#x",
            "blugit://00-00-00-00-00-00",
        ] {
            assert!(value.parse::<Endpoint>().is_err(), "{value}");
        }
    }
}
