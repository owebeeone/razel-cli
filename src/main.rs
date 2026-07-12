//! `razel-cli` thin standalone bin. SKELETON: the socket transport does not exist yet, and the raw-OS
//! wall bans `std::env`/`std::process` outside the OS seam, so this stub neither reads argv nor sets an
//! exit code — it only proves the bin target links the client closure and nothing else (gate G7).
//! The shipped multi-call binary is `razel-daemon`, which links this crate as a LIBRARY.

fn main() {
    let _codec = razel_cli::default_codec();
    println!(
        "razel-cli skeleton: the standalone client stub links the closure and nothing else — use the `razel` \
         binary (one client+daemon multi-call, like bazel): `razel build <pattern>`."
    );
}
