// Registers `--cfg loom` as an expected configuration so a normal workspace build
// (which never sets it) does not emit an `unexpected_cfgs` warning. The crate's
// contents live entirely behind `#[cfg(loom)]`; the model is run with
// `RUSTFLAGS="--cfg loom" cargo test -p loom_epoch_model --release`.
fn main() {
    println!("cargo::rustc-check-cfg=cfg(loom)");
}
