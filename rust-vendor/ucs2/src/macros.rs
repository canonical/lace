use crate::{ucs2_from_utf8_at_offset, Error};

/// Count the number of UCS-2 characters in a string. Return an error if
/// the string cannot be encoded in UCS-2.
pub const fn str_num_ucs2_chars(s: &str) -> Result<usize, Error> {
    let bytes = s.as_bytes();
    let len = bytes.len();

    let mut offset = 0;
    let mut num_ucs2_chars = 0;

    while offset < len {
        // SAFETY: `bytes` is valid UTF-8.
        match unsafe { ucs2_from_utf8_at_offset(bytes, offset) } {
            Ok(ch) => {
                offset += ch.num_bytes as usize;
                num_ucs2_chars += 1;
            }
            Err(err) => {
                return Err(err);
            }
        }
    }

    Ok(num_ucs2_chars)
}

/// Convert a `str` into a null-terminated UCS-2 character array.
pub const fn str_to_ucs2<const N: usize>(s: &str) -> Result<[u16; N], Error> {
    let bytes = s.as_bytes();
    let len = bytes.len();

    let mut output = [0; N];

    let mut output_offset = 0;
    let mut input_offset = 0;
    while input_offset < len {
        // SAFETY: `bytes` is valid UTF-8.
        match unsafe { ucs2_from_utf8_at_offset(bytes, input_offset) } {
            Ok(ch) => {
                if ch.val == 0 {
                    panic!("interior null character");
                } else {
                    output[output_offset] = ch.val;
                    output_offset += 1;
                    input_offset += ch.num_bytes as usize;
                }
            }
            Err(err) => {
                return Err(err);
            }
        }
    }

    // The output array must be one bigger than the converted string,
    // to leave room for the trailing null character.
    if output_offset + 1 != N {
        panic!("incorrect array length");
    }

    Ok(output)
}

/// Encode a string as UCS-2 with a trailing null character.
///
/// The encoding is done at compile time, so the result can be used in a
/// `const` item. The type returned by the macro is a `[u16; N]` array;
/// to avoid having to specify what `N` is in a `const` item, take a
/// reference and store it as `&[u16]`.
///
/// # Example
///
/// ```
/// use ucs2::ucs2_cstr;
///
/// const S: &[u16] = &ucs2_cstr!("abc");
/// assert_eq!(S, [97, 98, 99, 0]);
/// ```
#[macro_export]
macro_rules! ucs2_cstr {
    ($s:literal) => {{
        // Use `const` values here to force errors to happen at compile
        // time.

        const NUM_CHARS: usize = match $crate::str_num_ucs2_chars($s) {
            // Add one for the null char.
            Ok(num) => num + 1,
            Err(_) => panic!("input contains a character which cannot be represented in UCS-2"),
        };

        const VAL: [u16; NUM_CHARS] = match $crate::str_to_ucs2($s) {
            Ok(val) => val,
            // The string was already checked by `str_num_ucs2_chars`,
            // so this error is unreachable.
            Err(_) => {
                unreachable!();
            }
        };
        VAL
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_str_num_chars() {
        // Some of the strings here are from https://www.kermitproject.org/utf8.html.

        // One-byte chars.
        assert_eq!(str_num_ucs2_chars("abc"), Ok(3));
        // Two-byte chars.
        assert_eq!(str_num_ucs2_chars("Τη γλώσσα μου έδωσαν ελληνική"), Ok(29));
        // Three-byte chars.
        assert_eq!(str_num_ucs2_chars("ვეპხის ტყაოსანი შოთა რუსთაველი"), Ok(30));
        // Four-byte chars.
        assert_eq!(str_num_ucs2_chars("😎🔥"), Err(Error::MultiByte));
    }

    #[test]
    fn test_ucs2_cstr() {
        let s = ucs2_cstr!("abc");
        assert_eq!(s, [97, 98, 99, 0]);
    }
}
