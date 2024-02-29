use std::path::{absolute, PathBuf};

use tracing::debug;

use super::request::ProofInstance;

#[derive(Debug, Default, Clone)]
pub struct Context {
    /// guest executable path
    pub guest_path: PathBuf,
    /// cache for public input
    pub cache_path: PathBuf,

    pub max_caches: usize,

    pub l1_cache_file: Option<PathBuf>,
    
    pub l2_cache_file: Option<PathBuf>,

}

impl Context {
    pub fn update_cache_path(&mut self, block_no: u64) {
        if self.l1_cache_file.is_none() {
            let file_name = format!("{}.l1.json.gz", block_no);
            self.l1_cache_file = Some(cache_path.join(file_name));
        }
        if self.l2_cache_file.is_some() {
            let file_name = format!("{}.l2.json.gz", block_no);
            self.l2_cache_file = Some(cache_path.join(file_name));
        }
    }

    pub fn guest_executable_path(&self, proof_instance: ProofInstance) -> PathBuf {
        match proof_instance {
            ProofInstance::Succinct => todo!(),
            ProofInstance::PseZk => todo!(),
            ProofInstance::Powdr => todo!(),
            ProofInstance::Sgx(_) => self.guest_path.join("sgx").join(RAIKO_GUEST_EXECUTABLE),
            ProofInstance::Risc0 => todo!(),
        }
    }

    pub async fn remove_cache_file(&self) -> Result<()> {
        let remove = |file: PathBuf| {
            tokio::fs::remove_file(file).await.or_else(|e| {
                if e.kind() == ::std::io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(e)
                }
            })
        };
        self.l1_cache_file.map_or(Ok(()), |f| remove(f))?;
        self.l2_cache_file.map_or(Ok(()), |f| remove(f))?;
        Ok(())
    }

    
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
