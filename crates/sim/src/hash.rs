//! A small, fixed checksum (FNV-1a, 64 bit) for comparing simulation states between runs.
//! Unlike the standard library's hasher, its output is fixed forever.

#[derive(Clone, Copy, Debug)]
pub struct Checksum(u64);

impl Default for Checksum {
    fn default() -> Self {
        Checksum(0xCBF2_9CE4_8422_2325)
    }
}

impl Checksum {
    pub fn bytes(&mut self, data: &[u8]) {
        for &b in data {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01B3);
        }
    }
    pub fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }
    pub fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes());
    }
    pub fn f32(&mut self, v: f32) {
        self.u32(v.to_bits());
    }
    pub fn finish(self) -> u64 {
        self.0
    }
}
