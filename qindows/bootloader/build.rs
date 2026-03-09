/// Bootloader build script.
///
/// Instructs Cargo to re-run this build script (and thus re-compile
/// the bootloader) whenever the embedded Qernel ELF blob changes.
/// Without this, Cargo would cache a stale bootloader even after
/// the Qernel binary is rebuilt.
fn main() {
    // Bug 12 Fix: watch the embedded kernel blob for changes.
    println!("cargo:rerun-if-changed=blob/qernel.elf");

    // Also watch the bootloader source itself.
    println!("cargo:rerun-if-changed=src/main.rs");
}
