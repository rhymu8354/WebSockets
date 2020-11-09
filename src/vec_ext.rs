pub trait VecExt {
    fn push_word(
        &mut self,
        word: usize,
        bits: usize,
    );
}

impl VecExt for Vec<u8> {
    fn push_word(
        &mut self,
        word: usize,
        bits: usize,
    ) {
        let bytes = bits / 8;
        for bytes in (0..bytes).rev() {
            #[allow(clippy::cast_possible_truncation)]
            self.push(((word >> (bytes * 8)) & 0xFF) as u8);
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn push_word() {
        let mut buf = Vec::new();
        buf.push_word(0x1234, 16);
        buf.push_word(0x56, 8);
        buf.push_word(0xbaad_f00d, 0);
        buf.push_word(0xab_cd_ef, 24);
        assert_eq!(6, buf.len());
        assert_eq!(0x12, buf[0]);
        assert_eq!(0x34, buf[1]);
        assert_eq!(0x56, buf[2]);
        assert_eq!(0xab, buf[3]);
        assert_eq!(0xcd, buf[4]);
        assert_eq!(0xef, buf[5]);
    }
}
