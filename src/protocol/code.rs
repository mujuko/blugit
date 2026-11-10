use anyhow::{Result, ensure};
use rand::{RngExt, rand_core::UnwrapErr, rngs::SysRng};
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Code(String);
impl Code {
    pub(super) fn generate() -> Self {
        Self(format!(
            "{:06}",
            UnwrapErr(SysRng).random_range(0..1_000_000u32)
        ))
    }
    pub fn parse(value: &str) -> Result<Self> {
        ensure!(
            value.len() == 6 && value.bytes().all(|b| b.is_ascii_digit()),
            "enter exactly six digits"
        );
        Ok(Self(value.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codes_keep_leading_zeroes_and_reject_other_input() -> Result<()> {
        assert_eq!(Code::parse("000123")?.as_str(), "000123");
        for bad in ["12345", "1234567", "１２３４５６", "123 45", "12345\n"] {
            assert!(Code::parse(bad).is_err());
        }
        for _ in 0..20 {
            let c = Code::generate();
            assert!(Code::parse(c.as_str()).is_ok());
        }
        Ok(())
    }
}
