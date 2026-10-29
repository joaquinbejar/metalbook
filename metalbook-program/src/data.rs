//! Little-endian reader for instruction data.

use crate::error::MetalbookError;

/// Reads fixed-size little-endian fields from the front of a byte slice.
#[derive(Debug, Clone, Copy)]
pub struct Reader<'a> {
    rest: &'a [u8],
}

impl<'a> Reader<'a> {
    /// A reader over `data`.
    #[must_use]
    pub const fn new(data: &'a [u8]) -> Self {
        Self { rest: data }
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N], MetalbookError> {
        let (head, rest) = self
            .rest
            .split_first_chunk::<N>()
            .ok_or(MetalbookError::InvalidInstructionData)?;
        self.rest = rest;
        Ok(*head)
    }

    /// One byte.
    ///
    /// # Errors
    ///
    /// `InvalidInstructionData` if the data is exhausted.
    pub fn u8(&mut self) -> Result<u8, MetalbookError> {
        let [b] = self.take::<1>()?;
        Ok(b)
    }

    /// An `i8`.
    ///
    /// # Errors
    ///
    /// `InvalidInstructionData` if the data is exhausted.
    pub fn i8(&mut self) -> Result<i8, MetalbookError> {
        self.take().map(i8::from_le_bytes)
    }

    /// A `u16`.
    ///
    /// # Errors
    ///
    /// `InvalidInstructionData` if the data is exhausted.
    pub fn u16(&mut self) -> Result<u16, MetalbookError> {
        self.take().map(u16::from_le_bytes)
    }

    /// A `u32`.
    ///
    /// # Errors
    ///
    /// `InvalidInstructionData` if the data is exhausted.
    pub fn u32(&mut self) -> Result<u32, MetalbookError> {
        self.take().map(u32::from_le_bytes)
    }

    /// A `u64`.
    ///
    /// # Errors
    ///
    /// `InvalidInstructionData` if the data is exhausted.
    pub fn u64(&mut self) -> Result<u64, MetalbookError> {
        self.take().map(u64::from_le_bytes)
    }

    /// An `i64`.
    ///
    /// # Errors
    ///
    /// `InvalidInstructionData` if the data is exhausted.
    pub fn i64(&mut self) -> Result<i64, MetalbookError> {
        self.take().map(i64::from_le_bytes)
    }

    /// 32 raw bytes.
    ///
    /// # Errors
    ///
    /// `InvalidInstructionData` if the data is exhausted.
    pub fn bytes32(&mut self) -> Result<[u8; 32], MetalbookError> {
        self.take()
    }

    /// Succeeds only if every byte was read.
    ///
    /// # Errors
    ///
    /// `InvalidInstructionData` if bytes remain.
    pub fn finish(self) -> Result<(), MetalbookError> {
        if self.rest.is_empty() {
            Ok(())
        } else {
            Err(MetalbookError::InvalidInstructionData)
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::disallowed_macros
)]
mod tests {
    use super::Reader;
    use crate::error::MetalbookError;

    #[test]
    fn test_reader_reads_le_fields_and_rejects_short_or_long_data() {
        let data = [1u8, 2, 0, 3, 0, 0, 0];
        let mut r = Reader::new(&data);
        assert_eq!(r.u8(), Ok(1));
        assert_eq!(r.u16(), Ok(2));
        assert_eq!(r.u32(), Ok(3));
        assert_eq!(r.finish(), Ok(()));
        let mut r = Reader::new(&data);
        assert_eq!(r.u64(), Err(MetalbookError::InvalidInstructionData));
        let r = Reader::new(&data);
        assert_eq!(r.finish(), Err(MetalbookError::InvalidInstructionData));
    }
}
