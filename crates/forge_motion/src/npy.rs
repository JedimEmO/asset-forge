//! Just enough `NumPy` `.npy` to read what ARDY writes.
//!
//! A `.npz` is a zip of `.npy` members, and ARDY's are **stored, not
//! compressed** — so this is a header parse and a byte reinterpret. Pulling in
//! a full ndarray stack to read six arrays of known shape would be the largest
//! dependency in the crate.

/// A parsed `.npy` array: little-endian `f32`, C order.
#[derive(Debug, Clone)]
pub struct Array {
    /// Dimensions, outermost first.
    pub shape: Vec<usize>,
    /// Elements in C (row-major) order.
    pub data: Vec<f32>,
}

impl Array {
    /// Total element count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Whether the array holds no elements.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// Why an `.npy` member could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NpyError {
    /// Missing or wrong magic bytes.
    NotNpy,
    /// The header was truncated or not valid ASCII.
    BadHeader,
    /// A dtype this reader does not handle.
    ///
    /// Deliberately narrow: ARDY writes `<f4` for motion, `<i8` for fps and a
    /// unicode string for the prompt. Anything else means the format changed
    /// and silently coercing it would be worse than refusing.
    UnsupportedDtype(String),
    /// The declared shape does not match the number of bytes present.
    ShapeMismatch {
        /// Elements implied by the shape.
        expected: usize,
        /// Elements actually present.
        got: usize,
    },
}

impl std::fmt::Display for NpyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotNpy => f.write_str("not a .npy array (bad magic)"),
            Self::BadHeader => f.write_str("unreadable .npy header"),
            Self::UnsupportedDtype(d) => write!(f, "unsupported .npy dtype {d}"),
            Self::ShapeMismatch { expected, got } => {
                write!(f, "shape implies {expected} elements but {got} are present")
            }
        }
    }
}

impl std::error::Error for NpyError {}

/// Split an `.npy` blob into its header dict and its raw data bytes.
fn split(bytes: &[u8]) -> Result<(&str, &[u8]), NpyError> {
    if bytes.len() < 10 || &bytes[0..6] != b"\x93NUMPY" {
        return Err(NpyError::NotNpy);
    }
    // v1 stores the header length as u16; v2+ uses u32. ARDY writes v1, but
    // handling both is two lines and avoids a mystery failure after an upgrade.
    let (header_len, start) = if bytes[6] >= 2 {
        let len = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        (len, 12)
    } else {
        (u16::from_le_bytes([bytes[8], bytes[9]]) as usize, 10)
    };
    let end = start + header_len;
    if end > bytes.len() {
        return Err(NpyError::BadHeader);
    }
    let header = std::str::from_utf8(&bytes[start..end]).map_err(|_| NpyError::BadHeader)?;
    Ok((header, &bytes[end..]))
}

/// Pull `key`'s value out of the header's Python dict literal.
fn field<'a>(header: &'a str, key: &str) -> Option<&'a str> {
    let at = header.find(&format!("'{key}'"))?;
    let rest = &header[at + key.len() + 2..];
    let colon = rest.find(':')?;
    Some(rest[colon + 1..].trim_start())
}

/// Parse the header's `shape` tuple.
fn parse_shape(header: &str) -> Result<Vec<usize>, NpyError> {
    let shape_field = field(header, "shape").ok_or(NpyError::BadHeader)?;
    let open = shape_field.find('(').ok_or(NpyError::BadHeader)?;
    let close = shape_field.find(')').ok_or(NpyError::BadHeader)?;
    Ok(shape_field[open + 1..close]
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect())
}

/// Parse an `.npy` array of `f32`.
///
/// # Errors
///
/// Returns [`NpyError`] if the magic, header, dtype or length is wrong.
pub fn read_f32(bytes: &[u8]) -> Result<Array, NpyError> {
    let (header, data) = split(bytes)?;

    let descr = field(header, "descr").ok_or(NpyError::BadHeader)?;
    let descr = descr.trim_start_matches(['\'', '"']);
    if !descr.starts_with("<f4") {
        return Err(NpyError::UnsupportedDtype(
            descr.chars().take(4).collect::<String>(),
        ));
    }
    // Fortran order would need a transpose; ARDY never writes it, and reading
    // it as C order would silently scramble the joints.
    if field(header, "fortran_order").is_some_and(|v| v.starts_with("True")) {
        return Err(NpyError::UnsupportedDtype("fortran_order=True".to_owned()));
    }

    let shape = parse_shape(header)?;

    let expected: usize = shape
        .iter()
        .product::<usize>()
        .max(usize::from(shape.is_empty()));
    let got = data.len() / 4;
    if got < expected {
        return Err(NpyError::ShapeMismatch { expected, got });
    }

    let mut values = Vec::with_capacity(expected);
    for chunk in data.chunks_exact(4).take(expected) {
        values.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Ok(Array {
        shape,
        data: values,
    })
}

/// A parsed boolean `.npy` array: numpy dtype `|b1`, C order.
///
/// A separate type rather than a generic [`Array`]: the crate reads exactly
/// the element types ARDY writes, and a type parameter would push generics
/// into every caller to serve two variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoolArray {
    /// Dimensions, outermost first.
    pub shape: Vec<usize>,
    /// Elements in C (row-major) order.
    pub data: Vec<bool>,
}

/// Parse an `.npy` array of booleans (dtype `|b1`), as ARDY writes
/// `foot_contacts`.
///
/// numpy stores each boolean as one byte, `\x00` or `\x01`; any non-zero byte
/// reads as `true`, which matches how numpy itself interprets a byte viewed
/// as `bool_`.
///
/// # Errors
///
/// Returns [`NpyError`] if the magic, header, dtype or length is wrong.
pub fn read_bool(bytes: &[u8]) -> Result<BoolArray, NpyError> {
    let (header, data) = split(bytes)?;

    let descr = field(header, "descr").ok_or(NpyError::BadHeader)?;
    let descr = descr.trim_start_matches(['\'', '"']);
    if !descr.starts_with("|b1") {
        return Err(NpyError::UnsupportedDtype(
            descr.chars().take(4).collect::<String>(),
        ));
    }
    // One byte per element makes Fortran order equally wrong for anything
    // multi-dimensional: contact columns would swap sides silently.
    if field(header, "fortran_order").is_some_and(|v| v.starts_with("True")) {
        return Err(NpyError::UnsupportedDtype("fortran_order=True".to_owned()));
    }

    let shape = parse_shape(header)?;
    let expected: usize = shape
        .iter()
        .product::<usize>()
        .max(usize::from(shape.is_empty()));
    if data.len() < expected {
        return Err(NpyError::ShapeMismatch {
            expected,
            got: data.len(),
        });
    }

    Ok(BoolArray {
        shape,
        data: data[..expected].iter().map(|b| *b != 0).collect(),
    })
}

/// Read a 0-d integer array, as ARDY writes `fps`.
///
/// # Errors
///
/// Returns [`NpyError`] if the header is unreadable or the dtype is not an
/// 8- or 4-byte little-endian signed integer.
pub fn read_scalar_int(bytes: &[u8]) -> Result<i64, NpyError> {
    let (header, data) = split(bytes)?;
    let descr = field(header, "descr")
        .ok_or(NpyError::BadHeader)?
        .trim_start_matches(['\'', '"']);
    if descr.starts_with("<i8") && data.len() >= 8 {
        let mut b = [0u8; 8];
        b.copy_from_slice(&data[..8]);
        Ok(i64::from_le_bytes(b))
    } else if descr.starts_with("<i4") && data.len() >= 4 {
        Ok(i64::from(i32::from_le_bytes([
            data[0], data[1], data[2], data[3],
        ])))
    } else {
        Err(NpyError::UnsupportedDtype(
            descr.chars().take(4).collect::<String>(),
        ))
    }
}

/// Read a 0-d little-endian UTF-32 string, as ARDY writes the prompt (`<U52`).
///
/// # Errors
///
/// Returns [`NpyError`] if the header is unreadable or the dtype is not `<U`.
pub fn read_scalar_str(bytes: &[u8]) -> Result<String, NpyError> {
    let (header, data) = split(bytes)?;
    let descr = field(header, "descr")
        .ok_or(NpyError::BadHeader)?
        .trim_start_matches(['\'', '"']);
    if !descr.starts_with("<U") {
        return Err(NpyError::UnsupportedDtype(
            descr.chars().take(4).collect::<String>(),
        ));
    }
    // numpy pads to the declared width with NULs; stop at the first one.
    let mut out = String::new();
    for chunk in data.chunks_exact(4) {
        let code = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        if code == 0 {
            break;
        }
        if let Some(c) = char::from_u32(code) {
            out.push(c);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Assemble a v1 `.npy` blob byte-for-byte the way `np.save` does, so the
    /// parser is exercised against the real wire format rather than a mock of
    /// this module's own assumptions.
    fn npy_blob(descr: &str, shape: &str, data: &[u8]) -> Vec<u8> {
        let header =
            format!("{{'descr': '{descr}', 'fortran_order': False, 'shape': {shape}, }}\n");
        let mut out = Vec::new();
        out.extend_from_slice(b"\x93NUMPY\x01\x00");
        out.extend_from_slice(
            &u16::try_from(header.len())
                .expect("test header fits u16")
                .to_le_bytes(),
        );
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(data);
        out
    }

    #[test]
    fn read_bool_parses_a_b1_matrix() {
        let blob = npy_blob("|b1", "(2, 4)", &[1, 0, 0, 1, 0, 1, 1, 0]);
        let arr = read_bool(&blob).expect("parse");
        assert_eq!(arr.shape, vec![2, 4]);
        assert_eq!(
            arr.data,
            vec![true, false, false, true, false, true, true, false]
        );
    }

    #[test]
    fn read_bool_refuses_other_dtypes() {
        let blob = npy_blob("<f4", "(2,)", &[0; 8]);
        assert!(matches!(
            read_bool(&blob),
            Err(NpyError::UnsupportedDtype(d)) if d.starts_with("<f4")
        ));
    }

    #[test]
    fn read_bool_refuses_truncated_data() {
        let blob = npy_blob("|b1", "(3, 4)", &[1, 0, 0, 1, 0]);
        assert_eq!(
            read_bool(&blob),
            Err(NpyError::ShapeMismatch {
                expected: 12,
                got: 5
            })
        );
    }
}
