use core::cmp::min;

pub fn string_to_bytes32(input: &[u8]) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    let len = min(input.len(), 32);
    bytes[..len].copy_from_slice(&input[..len]);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn string_to_bytes32_test() {
        let input = "";
        let byte = string_to_bytes32(input.as_bytes());
        assert_eq!(
            byte,
            [
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0
            ]
        );
    }
}
