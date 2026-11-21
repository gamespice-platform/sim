//! CRC-32 (the zlib polynomial, reflected) by slicing-by-8: the same value as dot-map's
//! `GSMapVpk.crc32`, which stays the specification and the fallback.

const fn tables() -> [[u32; 256]; 8] {
    let mut t = [[0u32; 256]; 8];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        t[0][n] = c;
        n += 1;
    }
    let mut k = 1;
    while k < 8 {
        let mut n = 0;
        while n < 256 {
            let c = t[k - 1][n];
            t[k][n] = t[0][(c & 0xFF) as usize] ^ (c >> 8);
            n += 1;
        }
        k += 1;
    }
    t
}

static T: [[u32; 256]; 8] = tables();

/// The CRC-32 of `data`.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    let (chunks, rest) = data.as_chunks::<8>();
    for c in chunks {
        let a = u32::from_le_bytes([c[0], c[1], c[2], c[3]]) ^ crc;
        let b = u32::from_le_bytes([c[4], c[5], c[6], c[7]]);
        crc = T[7][(a & 0xFF) as usize]
            ^ T[6][((a >> 8) & 0xFF) as usize]
            ^ T[5][((a >> 16) & 0xFF) as usize]
            ^ T[4][(a >> 24) as usize]
            ^ T[3][(b & 0xFF) as usize]
            ^ T[2][((b >> 8) & 0xFF) as usize]
            ^ T[1][((b >> 16) & 0xFF) as usize]
            ^ T[0][(b >> 24) as usize];
    }
    for &x in rest {
        crc = T[0][((crc ^ u32::from(x)) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

#[cfg(test)]
mod tests {
    use super::crc32;

    #[test]
    fn known_values() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(
            crc32(b"The quick brown fox jumps over the lazy dog"),
            0x414F_A339
        );
    }

    #[test]
    fn every_tail_length_matches_bytewise() {
        let data: Vec<u8> = (0..200u32).map(|i| (i * 31 + 7) as u8).collect();
        for n in 0..data.len() {
            let mut c = 0xFFFF_FFFFu32;
            for &x in &data[..n] {
                c ^= u32::from(x);
                for _ in 0..8 {
                    c = if c & 1 != 0 {
                        0xEDB8_8320 ^ (c >> 1)
                    } else {
                        c >> 1
                    };
                }
            }
            assert_eq!(crc32(&data[..n]), c ^ 0xFFFF_FFFF, "len {n}");
        }
    }
}
