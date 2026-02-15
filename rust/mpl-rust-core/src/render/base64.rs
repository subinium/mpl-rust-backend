/// Decode a standard base64 string into raw bytes.
pub(crate) fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    #[inline]
    fn decode_char(c: u8) -> Result<u8, String> {
        match c {
            b'A'..=b'Z' => Ok(c - b'A'),
            b'a'..=b'z' => Ok(c - b'a' + 26),
            b'0'..=b'9' => Ok(c - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err(format!("invalid base64 character: {}", c as char)),
        }
    }

    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut q = [0u8; 4];
    let mut qlen = 0usize;

    for &b in bytes {
        if b == b'=' {
            break;
        }
        if b == b'\n' || b == b'\r' || b == b' ' || b == b'\t' {
            continue;
        }
        q[qlen] = decode_char(b)?;
        qlen += 1;
        if qlen == 4 {
            output.push((q[0] << 2) | (q[1] >> 4));
            output.push((q[1] << 4) | (q[2] >> 2));
            output.push((q[2] << 6) | q[3]);
            qlen = 0;
        }
    }

    match qlen {
        0 => {}
        2 => {
            output.push((q[0] << 2) | (q[1] >> 4));
        }
        3 => {
            output.push((q[0] << 2) | (q[1] >> 4));
            output.push((q[1] << 4) | (q[2] >> 2));
        }
        _ => return Err("invalid base64 length".to_string()),
    }

    Ok(output)
}

/// Encode raw bytes into a standard base64 string.
pub(crate) fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);

        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[((b0 & 0b0000_0011) << 4 | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((b1 & 0b0000_1111) << 2 | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(b2 & 0b0011_1111) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}
