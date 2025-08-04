use alloy_primitives::{B256, U256, keccak256};
use alloy_trie::Nibbles;
use std::{
    env,
    fs::File,
    io::Write,
    path::Path,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

/// The number of nibbles to use for the prefix.
const NIBBLES: usize = 5;
/// The total number of unique prefixes for the given number of nibbles (16^N).
const PREFIX_COUNT: usize = 16usize.pow(NIBBLES as u32);

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("preimages.bin");
    println!("cargo:rerun-if-changed=build.rs");

    let table: Arc<Vec<OnceLock<u64>>> =
        Arc::new((0..PREFIX_COUNT).map(|_| OnceLock::new()).collect());
    let found = Arc::new(AtomicUsize::new(0));

    thread::scope(|s| {
        let threads = thread::available_parallelism().unwrap().get();
        for tid in 0..threads {
            let table = table.clone();
            let found = found.clone();

            s.spawn(move || {
                let mut nonce = tid as u64;
                while found.load(Ordering::Relaxed) < PREFIX_COUNT {
                    let hash = keccak256(B256::from(U256::from(nonce)));

                    let nibbles = Nibbles::unpack(hash);
                    // Calculate the little-endian index from the first N nibbles of the hash.
                    let idx = (0..NIBBLES)
                        .map(|i| nibbles.get(i).unwrap() as usize)
                        .rfold(0, |a, n| (a << 4) | n);

                    if table[idx].set(nonce).is_ok() {
                        found.fetch_add(1, Ordering::Relaxed);
                    }
                    nonce += threads as u64;
                }
            });
        }
    });

    let mut file = File::create(&dest_path).expect("Could not create file");
    for cell in table.iter() {
        let nonce_bytes = cell.get().unwrap().to_le_bytes();
        file.write_all(&nonce_bytes).expect("Failed to write to file");
    }
}
