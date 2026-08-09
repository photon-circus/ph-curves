$ErrorActionPreference = "Continue"

function Invoke-Cargo {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Arguments)

    & cargo @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "cargo $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
    }
}

Invoke-Cargo fmt --all --check
Invoke-Cargo test
Invoke-Cargo test --features gen
# `gen` is the build.rs library API only. The CLI binary sits behind
# `gen-cli`, so without this line `--all-targets` silently stops covering
# src/bin/gen/main.rs.
Invoke-Cargo test --features gen-cli

$previousRustFlags = $env:RUSTFLAGS
try {
    $env:RUSTFLAGS = "-Dwarnings"
    Invoke-Cargo clippy --all-targets
    Invoke-Cargo clippy --all-targets --features gen
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

$xtensaTargets = @(
    "xtensa-esp32-none-elf",
    "xtensa-esp32s2-none-elf",
    "xtensa-esp32s3-none-elf"
)

foreach ($target in $xtensaTargets) {
    Invoke-Cargo +esp build --target $target -Zbuild-std=core
}

Invoke-Cargo package --allow-dirty
