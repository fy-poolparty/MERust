//! Little-endian cursor over a byte slice.

use std::fmt;

#[derive(Debug)]
pub struct ReadError {
    pub offset: usize,
    pub what: String,
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "at 0x{:x}: {}", self.offset, self.what)
    }
}

impl std::error::Error for ReadError {}

pub type Result<T> = std::result::Result<T, ReadError>;

#[derive(Clone)]
pub struct Reader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn at(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos }
    }

    pub fn err<T>(&self, what: impl Into<String>) -> Result<T> {
        Err(ReadError { offset: self.pos, what: what.into() })
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.remaining() < n {
            return self.err(format!("need {n} bytes, have {}", self.remaining()));
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.bytes(n).map(|_| ())
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }

    pub fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }

    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
    }

    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    pub fn vec3(&mut self) -> Result<[f32; 3]> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }

    /// Array length with a sanity bound so corrupt data fails fast instead of allocating.
    pub fn count(&mut self, max: usize) -> Result<usize> {
        let n = self.i32()?;
        if n < 0 || n as usize > max {
            return self.err(format!("implausible count {n}"));
        }
        Ok(n as usize)
    }

    /// Unreal FString: positive length = ANSI incl. NUL, negative = UTF-16 incl. NUL.
    pub fn fstring(&mut self) -> Result<String> {
        let n = self.i32()?;
        if n == 0 {
            return Ok(String::new());
        }
        if n > 0 {
            let b = self.bytes(n as usize)?;
            let b = b.strip_suffix(&[0]).unwrap_or(b);
            Ok(b.iter().map(|&c| c as char).collect())
        } else {
            let n = (-n) as usize;
            let b = self.bytes(n * 2)?;
            let u: Vec<u16> = b.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            let u = u.strip_suffix(&[0]).unwrap_or(&u);
            Ok(String::from_utf16_lossy(u))
        }
    }
}
