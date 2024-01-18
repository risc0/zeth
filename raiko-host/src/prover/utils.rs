use std::path::{Path, PathBuf};

use super::consts::RAIKO_GUEST_EXECUTABLE;

pub fn cache_file_path(cache_path: &Path, block_no: u64, is_l1: bool) -> PathBuf {
    let prefix = if is_l1 { "l1" } else { "l2" };
    let file_name = format!("{}.{}.json.gz", block_no, prefix);
    cache_path.join(file_name)
}

pub fn guest_executable_path(guest_path: &Path, proof_type: &str) -> PathBuf {
    guest_path.join(proof_type).join(RAIKO_GUEST_EXECUTABLE)
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_file_prefix() {
        let path = std::path::Path::new("/tmp/ethereum/1234.l1.json.gz");
        let prefix = path.file_prefix().unwrap();
        assert_eq!(prefix, "1234");
    }
}
