$ErrorActionPreference = "Continue"

# Incremental compilation makes this gate flaky on Windows: rustc fails to
# finalize `target/debug/incremental` ("Access is denied", os error 5) when a
# scanner or a previous run still holds a handle, and `cargo test` exits 101
# with every test reported as passing. The feature it lands on varies between
# runs, so the failure reads as a real, moving defect. A gate that randomly
# reports red trains you to re-run it instead of read it. CI builds fresh and
# gains nothing from incremental anyway.
$env:CARGO_INCREMENTAL = "0"

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
    # docs.rs publishes with features = ["gen-lib"] ([package.metadata.docs.rs]).
    # Without this flag, rustdoc never compiles src/gen, so a broken intra-doc
    # link there cannot fail the gate.
    Invoke-Cargo doc --no-deps --features gen-lib
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
    Invoke-Cargo build --example no_std_generated_fixtures --target $target
}

# Builds the sysroot from `core` alone: if anything on the default-feature path
# reached for `alloc` or `std`, this fails. A plain --target build only proves
# no-std; this is the no-alloc proof.
$coreOnlyTargets = @(
    "thumbv7em-none-eabi",
    "thumbv6m-none-eabi",
    "riscv32imc-unknown-none-elf",
    "msp430-none-elf"
)

foreach ($target in $coreOnlyTargets) {
    Invoke-Cargo +nightly build --target $target -Z build-std=core
    Invoke-Cargo +nightly build --example no_std_generated_fixtures --target $target -Z build-std=core
}

$xtensaTargets = @(
    "xtensa-esp32-none-elf",
    "xtensa-esp32s2-none-elf",
    "xtensa-esp32s3-none-elf"
)

foreach ($target in $xtensaTargets) {
    Invoke-Cargo +esp build --target $target -Zbuild-std=core
    Invoke-Cargo +esp build --example no_std_generated_fixtures --target $target -Zbuild-std=core
}

# Dependency policy: advisories, licences, bans, sources.
if (Get-Command cargo-deny -ErrorAction SilentlyContinue) {
    Invoke-Cargo deny check
} else {
    Write-Warning "cargo-deny not installed; skipping dependency policy. Install with 'cargo install cargo-deny'. CI runs it regardless."
}

Invoke-Cargo package --allow-dirty
