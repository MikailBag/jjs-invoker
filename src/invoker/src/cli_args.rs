use std::str::FromStr;

#[derive(thiserror::Error, Debug)]
pub enum ParseIdRangeError {
    #[error("string should contain exactly one ':' occurrence")]
    UnexpectedColonCount,
    #[error("invalid number")]
    BadNumber(#[from] std::num::ParseIntError),
    #[error("`low` must be less than `high`")]
    BadRange,
}

#[derive(Debug)]
pub(crate) struct IdRange {
    pub(crate) low: u32,
    pub(crate) high: u32,
}

impl FromStr for IdRange {
    type Err = ParseIdRangeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.chars().filter(|ch| *ch == ':').count() != 1 {
            return Err(ParseIdRangeError::UnexpectedColonCount);
        }
        let mut iter = s.split(':');
        let low = iter.next().unwrap();
        let high = iter.next().unwrap();
        let low: u32 = low.parse()?;
        let high: u32 = high.parse()?;
        if low >= high {
            return Err(ParseIdRangeError::BadRange);
        }
        Ok(IdRange { low, high })
    }
}
