fn main() {
    println!(
        "cargo:warning=woven-lite: ext-image-copy-capture-v1 backend not yet wired. \
         Window cards are Niri-only (zwlr) until dual-backend lands in woven-protocols. \
         Remove this warning in build.rs once woven-protocols::detect_backend() exists."
    );
}
