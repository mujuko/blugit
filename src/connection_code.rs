//! Human-readable routing code: 48 address bits + 5 channel bits + CRC-5.
use crate::endpoint::{Address, Channel, Contact};
use anyhow::{Result, ensure};
use std::{fmt, str::FromStr};

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConnectionCode(pub Contact);

fn checksum(symbols: &[u8]) -> u8 {
    let mut crc = 0x1f;
    for symbol in symbols {
        for bit in (0..5).rev() {
            let feedback = ((crc >> 4) ^ (symbol >> bit)) & 1;
            crc = (crc << 1) & 31;
            if feedback != 0 {
                crc ^= 0x05;
            }
        }
    }
    crc
}
impl ConnectionCode {
    fn symbols(self) -> [u8; 12] {
        let mut payload = (self.0.address.as_u64() << 5) | u64::from(self.0.channel.get());
        let mut symbols = [0; 12];
        for symbol in symbols[..11].iter_mut().rev() {
            *symbol = (payload & 31) as u8;
            payload >>= 5;
        }
        symbols[11] = checksum(&symbols[..11]);
        symbols
    }
}
impl fmt::Display for ConnectionCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, symbol) in self.symbols().iter().enumerate() {
            if i == 4 || i == 8 {
                f.write_str("-")?;
            }
            write!(f, "{}", ALPHABET[*symbol as usize] as char)?;
        }
        Ok(())
    }
}
impl FromStr for ConnectionCode {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self> {
        let bytes = value.as_bytes();
        ensure!(
            bytes.len() == 12 || (bytes.len() == 14 && bytes[4] == b'-' && bytes[9] == b'-'),
            "connection code must be 12 letters/digits, optionally XXXX-XXXX-XXXX"
        );
        let mut symbols = Vec::with_capacity(12);
        for (i, byte) in bytes.iter().enumerate() {
            if bytes.len() == 14 && (i == 4 || i == 9) {
                continue;
            }
            let byte = match byte.to_ascii_uppercase() {
                b'O' => b'0',
                b'I' | b'L' => b'1',
                b => b,
            };
            let symbol = ALPHABET
                .iter()
                .position(|b| *b == byte)
                .ok_or_else(|| anyhow::anyhow!("invalid character in connection code"))?;
            symbols.push(symbol as u8);
        }
        ensure!(
            checksum(&symbols[..11]) == symbols[11],
            "connection code checksum mismatch; check with the host"
        );
        let payload = symbols[..11]
            .iter()
            .fold(0u64, |n, b| (n << 5) | u64::from(*b));
        ensure!(payload >> 53 == 0, "unsupported connection code version");
        Ok(Self(Contact {
            address: Address::from_u64(payload >> 5)?,
            channel: Channel::try_from((payload & 31) as u8)?,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_channel_and_address_bits_roundtrip() -> Result<()> {
        for address in [1, 0x00a750418d47, 0xffffffffffff] {
            for channel in 1..=30 {
                let code = ConnectionCode(Contact {
                    address: Address::from_u64(address)?,
                    channel: Channel::try_from(channel)?,
                });
                let text = code.to_string();
                assert_eq!(text.len(), 14);
                assert_eq!(text.parse::<ConnectionCode>()?, code);
                assert_eq!(
                    text.to_lowercase()
                        .replace('-', "")
                        .parse::<ConnectionCode>()?,
                    code
                );
                assert_eq!(
                    text.replace('0', "O")
                        .replace('1', "l")
                        .parse::<ConnectionCode>()?,
                    code
                );
            }
        }
        Ok(())
    }
    #[test]
    fn single_symbol_errors_and_adjacent_swaps_are_detected() -> Result<()> {
        let code = ConnectionCode(Contact {
            address: "00:A7:50:41:8D:47".parse()?,
            channel: Channel::try_from(7)?,
        });
        let text = code.to_string().replace('-', "").into_bytes();
        for i in 0..12 {
            for replacement in ALPHABET {
                if *replacement == text[i] {
                    continue;
                }
                let mut changed = text.clone();
                changed[i] = *replacement;
                assert!(
                    String::from_utf8(changed)?
                        .parse::<ConnectionCode>()
                        .is_err()
                );
            }
        }
        for i in 0..11 {
            if text[i] == text[i + 1] {
                continue;
            }
            let mut changed = text.clone();
            changed.swap(i, i + 1);
            assert!(
                String::from_utf8(changed)?
                    .parse::<ConnectionCode>()
                    .is_err()
            );
        }
        for invalid in [
            "",
            "123456",
            "1234--5678-ABCD",
            "1234_5678_ABCD",
            "あ23456789012",
            "UUUUUUUUUUUU",
        ] {
            assert!(invalid.parse::<ConnectionCode>().is_err());
        }
        // Checksum-valid but invalid routing data is still rejected.
        for payload in [0, 31, 32, 63, 1u64 << 53] {
            let mut symbols = [0; 12];
            let mut n = payload;
            for s in symbols[..11].iter_mut().rev() {
                *s = (n & 31) as u8;
                n >>= 5;
            }
            symbols[11] = checksum(&symbols[..11]);
            let text: String = symbols
                .iter()
                .map(|s| ALPHABET[*s as usize] as char)
                .collect();
            assert!(text.parse::<ConnectionCode>().is_err());
        }
        Ok(())
    }
}
