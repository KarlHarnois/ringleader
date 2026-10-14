pub struct FnvHash {
    value: u64,
}

impl FnvHash {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    const HALF_WIDTH: u32 = 32;

    pub fn of(bytes: &[u8]) -> Self {
        let value = bytes.iter().fold(Self::OFFSET_BASIS, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(Self::PRIME)
        });
        Self { value }
    }

    pub fn hex(&self) -> String {
        format!("{:016x}", self.value)
    }

    pub fn short_hex(&self) -> String {
        let folded = (self.value >> Self::HALF_WIDTH) ^ (self.value & u64::from(u32::MAX));
        format!("{folded:08x}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_bytes_hash_to_the_offset_basis() {
        assert_eq!(FnvHash::of(b"").hex(), "cbf29ce484222325");
    }

    #[test]
    fn bytes_hash_to_the_published_fnv1a_values() {
        assert_eq!(FnvHash::of(b"a").hex(), "af63dc4c8601ec8c");
        assert_eq!(FnvHash::of(b"foobar").hex(), "85944171f73967e8");
    }

    #[test]
    fn a_short_hex_folds_the_two_halves_together() {
        assert_eq!(FnvHash::of(b"").short_hex(), "4fd0bfc1");
        assert_eq!(FnvHash::of(b"a").short_hex(), "296230c0");
    }
}
