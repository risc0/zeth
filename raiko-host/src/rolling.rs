use std::{fs, path::Path, sync::Mutex};

use once_cell::sync::Lazy;

const L1_CACHE_FILE_SUFFIX: &str = ".l1.json.gz";
const L2_CACHE_FILE_SUFFIX: &str = ".l2.json.gz";

static MUTEX: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

// delete old cache files
pub fn prune_old_caches<T: AsRef<Path>>(cache_dir: T, max_blocks: usize) {
    let _guard = MUTEX.lock().unwrap();
    let cache_dir = cache_dir.as_ref();
    let files = fs::read_dir(cache_dir).map(|dir| {
        dir.filter_map(|entry| {
            let entry = entry.ok()?;
            let metadata = entry.metadata().ok()?;

            // the appender only creates files, not directories or symlinks,
            // so we should never delete a dir or symlink.
            if !metadata.is_file() {
                return None;
            }

            let filename = entry.file_name();
            // if the filename is not a UTF-8 string, skip it.
            let filename = filename.to_str()?;
            let block_no_l1 = filename.strip_suffix(L1_CACHE_FILE_SUFFIX);
            let block_no_l2 = filename.strip_suffix(L2_CACHE_FILE_SUFFIX);
            if block_no_l1.is_none() && block_no_l2.is_none() {
                return None;
            }
            let block_no = block_no_l1.or(block_no_l2)?;
            let block_no = block_no.parse::<usize>().ok()?;

            Some((entry, block_no))
        })
        .collect::<Vec<_>>()
    });

    let mut files = match files {
        Ok(files) => files,
        Err(error) => {
            eprintln!("Error reading the log directory/files: {}", error);
            return;
        }
    };
    let max_files = max_blocks * 2;
    if files.len() < max_files {
        return;
    }

    // sort the files by their creation timestamps.
    files.sort_by_key(|(_, block_no)| *block_no);

    for (file, _) in files.iter().take(files.len() - max_files) {
        if let Err(error) = fs::remove_file(file.path()) {
            eprintln!(
                "Failed to remove old log file {}: {}",
                file.path().display(),
                error
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_prune_old_caches() {
        prune_old_caches("./testdata/rolling_caches/", 2);
    }
}
