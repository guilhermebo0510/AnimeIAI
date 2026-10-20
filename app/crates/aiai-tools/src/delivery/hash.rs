//! SHA-256 (checksums do pacote de entrega) e CRC-32 (zip), sem dependência nova.

/// SHA-256 incremental.
pub struct Sha256 {
    h: [u32; 8],
    buf: [u8; 64],
    n: usize,
    total: u64,
}

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
    0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
    0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

impl Default for Sha256 {
    fn default() -> Self {
        Sha256 { h: [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19], buf: [0; 64], n: 0, total: 0 }
    }
}

impl Sha256 {
    fn block(&mut self, b: &[u8; 64]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([b[i * 4], b[i * 4 + 1], b[i * 4 + 2], b[i * 4 + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut bb, mut c, mut d, mut e, mut f, mut g, mut h] = self.h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & bb) ^ (a & c) ^ (bb & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = bb;
            bb = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in self.h.iter_mut().zip([a, bb, c, d, e, f, g, h]) {
            *x = x.wrapping_add(y);
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.total += data.len() as u64;
        if self.n > 0 {
            let take = (64 - self.n).min(data.len());
            self.buf[self.n..self.n + take].copy_from_slice(&data[..take]);
            self.n += take;
            data = &data[take..];
            if self.n == 64 {
                let b = self.buf;
                self.block(&b);
                self.n = 0;
            }
        }
        while data.len() >= 64 {
            let mut b = [0u8; 64];
            b.copy_from_slice(&data[..64]);
            self.block(&b);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.n = data.len();
        }
    }

    pub fn finish(mut self) -> [u8; 32] {
        let bits = self.total * 8;
        let mut pad = vec![0x80u8];
        while (self.n + pad.len()) % 64 != 56 {
            pad.push(0);
        }
        pad.extend_from_slice(&bits.to_be_bytes());
        let total = self.total;
        self.update(&pad);
        self.total = total;
        let mut out = [0u8; 32];
        for (i, x) in self.h.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
        }
        out
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::default();
    h.update(data);
    hex(&h.finish())
}

/// SHA-256 de um arquivo, lido em blocos (vídeos grandes não vão inteiros para a memória).
pub fn sha256_file(path: &std::path::Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::default();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finish()))
}

/// CRC-32 (IEEE) incremental, o do formato zip.
pub struct Crc32(u32);

fn table() -> &'static [u32; 256] {
    static T: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *e = c;
        }
        t
    })
}

impl Default for Crc32 {
    fn default() -> Self {
        Crc32(0xFFFF_FFFF)
    }
}

impl Crc32 {
    pub fn update(&mut self, data: &[u8]) {
        let t = table();
        for b in data {
            self.0 = t[((self.0 ^ *b as u32) & 0xFF) as usize] ^ (self.0 >> 8);
        }
    }
    pub fn finish(&self) -> u32 {
        !self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_the_published_vectors() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"), "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
        // blocos em pedaços de tamanhos diferentes dão o mesmo resultado
        let data: Vec<u8> = (0..1000u32).map(|i| (i * 7 % 251) as u8).collect();
        let whole = sha256_hex(&data);
        let mut h = Sha256::default();
        for chunk in data.chunks(37) {
            h.update(chunk);
        }
        assert_eq!(hex(&h.finish()), whole);
        // um milhão de 'a'
        let mut h = Sha256::default();
        for _ in 0..1000 {
            h.update(&[b'a'; 1000]);
        }
        assert_eq!(hex(&h.finish()), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
    }

    #[test]
    fn crc32_matches_the_check_value() {
        let mut c = Crc32::default();
        c.update(b"123456789");
        assert_eq!(c.finish(), 0xCBF4_3926);
    }
}
