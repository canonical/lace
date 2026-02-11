//! Utility functions for the UCS-2 character encoding.

#![no_std]
#![deny(missing_docs)]
#![deny(clippy::all)]

mod macros;

/// These need to be public for the `ucs2_cstr!` macro, but are not
/// intended to be called directly.
#[doc(hidden)]
pub use macros::{str_num_ucs2_chars, str_to_ucs2};

use bit_field::BitField;
use core::fmt::{self, Display, Formatter};

/// Possible errors returned by the API.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Error {
    /// Not enough space left in the output buffer.
    BufferOverflow,
    /// Input contained a character which cannot be represented in UCS-2.
    MultiByte,
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::BufferOverflow => f.write_str("output buffer is too small"),
            Self::MultiByte => {
                f.write_str("input contains a character which cannot be represented in UCS-2")
            }
        }
    }
}

type Result<T> = core::result::Result<T, Error>;

/// Value returned by `ucs2_from_utf8_at_offset`.
struct Ucs2CharFromUtf8 {
    /// UCS-2 character.
    val: u16,
    /// Number of bytes needed to encode the character in UTF-8.
    num_bytes: u8,
}

/// Get a UCS-2 character from a UTF-8 byte slice at the given offset.
///
/// # Safety
///
/// The input `bytes` must be valid UTF-8.
const unsafe fn ucs2_from_utf8_at_offset(bytes: &[u8], offset: usize) -> Result<Ucs2CharFromUtf8> {
    let len = bytes.len();
    let ch;
    let ch_len;

    if bytes[offset] & 0b1000_0000 == 0b0000_0000 {
        ch = bytes[offset] as u16;
        ch_len = 1;
    } else if bytes[offset] & 0b1110_0000 == 0b1100_0000 {
        // 2 byte codepoint
        if offset + 1 >= len {
            // safe: len is the length of bytes,
            // and bytes is a direct view into the
            // buffer of input, which in order to be a valid
            // utf-8 string _must_ contain `i + 1`.
            unsafe { core::hint::unreachable_unchecked() }
        }

        let a = (bytes[offset] & 0b0001_1111) as u16;
        let b = (bytes[offset + 1] & 0b0011_1111) as u16;
        ch = a << 6 | b;
        ch_len = 2;
    } else if bytes[offset] & 0b1111_0000 == 0b1110_0000 {
        // 3 byte codepoint
        if offset + 2 >= len || offset + 1 >= len {
            // safe: impossible utf-8 string.
            unsafe { core::hint::unreachable_unchecked() }
        }

        let a = (bytes[offset] & 0b0000_1111) as u16;
        let b = (bytes[offset + 1] & 0b0011_1111) as u16;
        let c = (bytes[offset + 2] & 0b0011_1111) as u16;
        ch = a << 12 | b << 6 | c;
        ch_len = 3;
    } else if bytes[offset] & 0b1111_0000 == 0b1111_0000 {
        return Err(Error::MultiByte); // UTF-16
    } else {
        // safe: impossible utf-8 string.
        unsafe { core::hint::unreachable_unchecked() }
    }

    Ok(Ucs2CharFromUtf8 {
        val: ch,
        num_bytes: ch_len,
    })
}

/// Encodes an input UTF-8 string into a UCS-2 string.
///
/// The returned `usize` represents the length of the returned buffer,
/// measured in 2-byte characters.
pub fn encode(input: &str, buffer: &mut [u16]) -> Result<usize> {
    let buffer_size = buffer.len();
    let mut i = 0;

    encode_with(input, |ch| {
        if i >= buffer_size {
            Err(Error::BufferOverflow)
        } else {
            buffer[i] = ch;
            i += 1;
            Ok(())
        }
    })?;

    Ok(i)
}

/// Encode UTF-8 string to UCS-2 with a custom callback function.
///
/// `output` is a function which receives every encoded character.
pub fn encode_with<F>(input: &str, mut output: F) -> Result<()>
where
    F: FnMut(u16) -> Result<()>,
{
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        // SAFETY: `bytes` is valid UTF-8.
        let ch = unsafe { ucs2_from_utf8_at_offset(bytes, i) }?;
        i += usize::from(ch.num_bytes);
        output(ch.val)?;
    }
    Ok(())
}

/// Decode UCS-2 string to UTF-8 with a custom callback function.
///
/// `output` is a function which receives every decoded character.
/// Due to the nature of UCS-2, the function can receive an UTF-8 character
/// of up to three bytes, for every input character.
pub fn decode_with<F>(input: &[u16], mut output: F) -> Result<usize>
where
    F: FnMut(&[u8]) -> Result<()>,
{
    let mut written = 0;

    for ch in input.iter() {
        /*
         * We need to find how many bytes of UTF-8 this UCS-2 code-point needs. Because UCS-2 can only encode
         * the Basic Multilingual Plane, a maximum of three bytes are needed.
         */
        if (0x000..0x0080).contains(ch) {
            output(&[*ch as u8])?;

            written += 1;
        } else if (0x0080..0x0800).contains(ch) {
            let first = 0b1100_0000 + ch.get_bits(6..11) as u8;
            let last = 0b1000_0000 + ch.get_bits(0..6) as u8;

            output(&[first, last])?;

            written += 2;
        } else {
            let first = 0b1110_0000 + ch.get_bits(12..16) as u8;
            let mid = 0b1000_0000 + ch.get_bits(6..12) as u8;
            let last = 0b1000_0000 + ch.get_bits(0..6) as u8;

            output(&[first, mid, last])?;

            written += 3;
        }
    }

    Ok(written)
}

/// Decode an input UCS-2 string into a UTF-8 string.
///
/// The returned `usize` represents the length of the returned buffer,
/// in bytes. Due to the nature of UCS-2, the output buffer could end up with
/// three bytes for every character in the input buffer.
pub fn decode(input: &[u16], output: &mut [u8]) -> Result<usize> {
    let buffer_size = output.len();
    let mut i = 0;

    decode_with(input, |bytes| {
        if bytes.len() == 1 {
            // Can be encoded in a single byte
            if i >= buffer_size {
                return Err(Error::BufferOverflow);
            }

            output[i] = bytes[0];

            i += 1;
        } else if bytes.len() == 2 {
            // Can be encoded two bytes
            if i + 1 >= buffer_size {
                return Err(Error::BufferOverflow);
            }

            output[i] = bytes[0];
            output[i + 1] = bytes[1];

            i += 2;
        } else if bytes.len() == 3 {
            // Can be encoded three bytes
            if i + 2 >= buffer_size {
                return Err(Error::BufferOverflow);
            }

            output[i] = bytes[0];
            output[i + 1] = bytes[1];
            output[i + 2] = bytes[2];

            i += 3;
        } else {
            unreachable!("More than three bytes per UCS-2 character.");
        }

        Ok(())
    })
}
