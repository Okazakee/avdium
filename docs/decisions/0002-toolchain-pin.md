# Decision: toolchain pin

Date: 2026-09-23
Status: accepted

## Context

The brief asks for Rust edition 2024 and an initial toolchain pin of `1.98.0`,
matching the researched reference, with availability confirmed and any change
recorded.

The host had only `stable-x86_64-unknown-linux-gnu` at `1.98.1` installed.

## Decision

Pin `1.98.0` in `rust-toolchain.toml`, and install it. Keep `1.98.1` on the
host as the default toolchain.

## Evidence

```
$ rustup toolchain install 1.98.0 --profile minimal
info: latest update on 2026-08-20 for version 1.98.0 (88d9e12ae 2026-08-18)
  1.98.0-x86_64-unknown-linux-gnu installed - rustc 1.98.0 (88d9e12ae 2026-08-18)
```

`1.98.0` is a published release and is installed. The pin is therefore real,
not aspirational.

## Consequences

Builds use `1.98.0` regardless of the host default. If a future dependency
requires a newer compiler, the pin moves deliberately and this record is
updated rather than silently deleted.
