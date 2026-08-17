$ErrorActionPreference = "Continue"

# Incremental compilation makes this gate flaky on Windows: rustc fails to
# finalize `target/debug/incremental` ("Access is denied", os error 5) when a
# scanner or a previous run still holds a handle, and `cargo test` exits 101
# with every test reported as passing. The feature it lands on varies between
# runs, so the failure reads as a real, moving defect. A gate that randomly
# reports red trains you to re-run it instead of read it. CI builds fresh and
# gains nothing from incremental anyway.
$env:CARGO_INCREMENTAL = "0"
# Match CI for every compilation step, not only clippy. A warning that appears
# in a test, example, target build, or packaged source is a release failure.
$env:RUSTFLAGS = "-Dwarnings"

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
if (Select-String -Path src/lib.rs -Pattern '^\s*(pub(\([^)]*\))?\s+)?extern\s+crate\s+std\b' -Quiet) {
    throw "src/lib.rs: must not link std at the crate root; keep it module-local under src/gen."
}
if (Select-String -Path src/lib.rs -Pattern '^\s*#\s*\[\s*macro_use(?:\s|\]|\()' -Quiet) {
    throw "src/lib.rs: #[macro_use] is forbidden at the crate root; it can expose allocating macros to runtime modules."
}
if (Select-String -Path src/gen/mod.rs -Pattern '^\s*#\s*\[\s*macro_use(?:\s|\]|\()' -Quiet) {
    throw "src/gen/mod.rs: link std explicitly without #[macro_use]; host modules import only what they use."
}
if (-not (Select-String -Path src/gen/mod.rs -Pattern '^\s*extern\s+crate\s+std;\s*$' -Quiet)) {
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
# Quote the separator: PowerShell otherwise consumes a bare `--` while binding
# this function call and accidentally asks Cargo itself for help.
Invoke-Cargo run --features gen --bin ph-curves-gen '--' --help

Invoke-Cargo clippy --all-targets
Invoke-Cargo clippy --all-targets --features gen-lib
Invoke-Cargo clippy --all-targets --features gen-cli

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

# Dependency policy is part of the release gate, not an optional convenience.
if (-not (Get-Command cargo-deny -ErrorAction SilentlyContinue)) {
    throw "cargo-deny is required. Install it with 'cargo install cargo-deny'."
}

# A library consumer on Cargo resolver 2 does not use this repository's
# Cargo.lock and may otherwise select a dependency whose MSRV is newer than the
# crate's. Build a fresh edition-2021 consumer with Rust 1.92 to prove the
# published dependency bounds remain satisfiable.
$tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$msrvProbe = Join-Path $tempRoot ("ph-curves-msrv-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path (Join-Path $msrvProbe "src") -Force | Out-Null
$repositoryRoot = (Resolve-Path -LiteralPath ".").Path.Replace("\", "/")
@"
[package]
name = "ph-curves-msrv-probe"
version = "0.0.0"
edition = "2021"
rust-version = "1.92.0"
resolver = "2"
publish = false

[dependencies]
ph-curves = { path = "$repositoryRoot" }
"@ | Set-Content -LiteralPath (Join-Path $msrvProbe "Cargo.toml") -Encoding utf8
@"
#![no_std]
use ph_curves::AffineTransform;
pub const IDENTITY: AffineTransform = match AffineTransform::new(1, 0, 1) {
    Ok(value) => value,
    Err(_) => panic!("identity coefficients are valid"),
};
"@ | Set-Content -LiteralPath (Join-Path $msrvProbe "src/lib.rs") -Encoding utf8
try {
    Invoke-Cargo +1.92.0 build --manifest-path (Join-Path $msrvProbe "Cargo.toml") --target thumbv7em-none-eabi
    $resolvedFixed = & cargo +1.92.0 tree --manifest-path (Join-Path $msrvProbe "Cargo.toml") -i fixed
    if ($LASTEXITCODE -ne 0 -or -not ($resolvedFixed -match '^fixed v1\.30\.')) {
        throw "fresh Rust 1.92 consumer did not resolve an MSRV-compatible fixed 1.30.x release"
    }
} finally {
    $resolvedProbe = [System.IO.Path]::GetFullPath($msrvProbe)
    if (-not $resolvedProbe.StartsWith($tempRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "refusing to clean unexpected MSRV probe path: $resolvedProbe"
    }
    if (Test-Path -LiteralPath $resolvedProbe) {
        Remove-Item -LiteralPath $resolvedProbe -Recurse -Force
    }
}
Invoke-Cargo deny --all-features check

$packageFiles = & cargo package --list --allow-dirty
if ($LASTEXITCODE -ne 0) {
    throw "cargo package --list failed with exit code $LASTEXITCODE"
}
foreach ($required in @(
    "assets/curves-u16.toml",
    "tests/fixtures/family_acceptance_generated.rs",
    "tests/fixtures/ntc_generated.rs",
    "tests/fixtures/observation_guards_generated.rs"
)) {
    if ($packageFiles -notcontains $required) {
        throw "packaged example dependency is missing: $required"
    }
}
Invoke-Cargo package --allow-dirty
