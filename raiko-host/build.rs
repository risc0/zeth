use sp1_helper::build_program;

fn main() {
    #[cfg(feature = "succinct")]
    build_program("../raiko-guests/succinct")
}
