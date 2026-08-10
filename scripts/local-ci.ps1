$ErrorActionPreference = "Continue"

function Invoke-Cargo {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Arguments)

    & cargo @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "cargo $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
    }
}

Invoke-Cargo fmt --all --check

# The runtime no-std / no-alloc guarantee. A feature-conditional `no_std` would
# let Cargo feature unification turn a firmware build into a std build.
if (Select-String -Path src/lib.rs -Pattern '^#!\[cfg_attr\(.*no_std' -Quiet) {
    throw "src/lib.rs: #![no_std] is feature-conditional; it must be unconditional."
}
if (-not (Select-String -Path src/lib.rs -Pattern '^#!\[no_std\]$' -Quiet)) {
    throw "src/lib.rs: missing an unconditional #![no_std]."
}
# `std` must be linked only inside src/gen, not at the crate root (and never
# via `#[macro_use]`, which would put the prelude on the runtime path).
if (Select-String -Path src/lib.rs -Pattern '^\s*extern crate std' -Quiet) {
    throw "src/lib.rs: must not link std at the crate root; keep it module-local under src/gen."
}
if (-not (Select-String -Path src/gen/mod.rs -Pattern '^\s*extern crate std;' -Quiet)) {
    throw "src/gen/mod.rs: missing module-local `extern crate std`."
}

Invoke-Cargo test
Invoke-Cargo test --features gen-lib
# `gen-lib` is the build.rs library API only. The CLI binary sits behind
# `gen-cli`, so without this line `--all-targets` silently stops covering
# src/bin/gen/main.rs.
Invoke-Cargo test --features gen-cli
# `gen` is the 0.1.x compatibility alias and must keep building the CLI.
Invoke-Cargo test --features gen
# Mirror CI `feature-compat`: the 0.1.x `gen` alias still runs the binary.
Invoke-Cargo run --features gen --bin ph-curves-gen -- --help

$previousRustFlags = $env:RUSTFLAGS
try {
    $env:RUSTFLAGS = "-Dwarnings"
    Invoke-Cargo clippy --all-targets
    Invoke-Cargo clippy --all-targets --features gen-lib
    Invoke-Cargo clippy --all-targets --features gen-cli
} finally {
    $env:RUSTFLAGS = $previousRustFlags
}

$previousRustdocFlags = $env:RUSTDOCFLAGS
try {
    $env:RUSTDOCFLAGS = "-Dwarnings"
    Invoke-Cargo doc --no-deps
} finally {
    $env:RUSTDOCFLAGS = $previousRustdocFlags
}

$targets = @(
    "thumbv7em-none-eabi",
    "thumbv6m-none-eabi",
    "riscv32imac-unknown-none-elf",
    "riscv32imc-unknown-none-elf",
    "wasm32-unknown-unknown"
)

foreach ($target in $targets) {
    Invoke-Cargo build --target $target
}

# Builds the sysroot from `core` alone: if anything on the default-feature path
# reached for `alloc` or `std`, this fails. A plain --target build only proves
# no-std; this is the no-alloc proof.
$coreOnlyTargets = @(
    "thumbv7em-none-eabi",
    "thumbv6m-none-eabi",
    "riscv32imc-unknown-none-elf"
)

foreach ($target in $coreOnlyTargets) {
    Invoke-Cargo +nightly build --target $target -Z build-std=core
}

$xtensaTargets = @(
    "xtensa-esp32-none-elf",
    "xtensa-esp32s2-none-elf",
    "xtensa-esp32s3-none-elf"
)

foreach ($target in $xtensaTargets) {
    Invoke-Cargo +esp build --target $target -Zbuild-std=core
}

Invoke-Cargo package --allow-dirty
