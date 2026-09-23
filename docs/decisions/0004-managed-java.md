# Decision: managed Java runtime

Date: 2026-09-23
Status: accepted (direction); exact artifact pin still unresolved

## Context

Avdium must not require a system-wide Java install. Providers may use a private
runtime or an already-compatible Java installation. The brief notes that
Android CLI ships an embedded runtime, and says not to reuse it unless it
exposes a supported executable and compatibility is demonstrated.

## Evidence

Host Java: OpenJDK `21.0.11` at `/usr/bin/java`.

Android CLI bundle at
`~/.android/cli/bundles/bffc6b8ca1741969f41b77766cfcbb76c42edb02`:

```
jre/release -> JAVA_VERSION="25.0.4"
jre/       -> conf/ legal/ lib/ release
jre/bin/   -> does not exist
```

The embedded runtime is a jlink image with **no `bin/` directory**, so it does
not expose a `java` executable. It cannot be used to run `sdkmanager` or
`avdmanager`.

The Adoptium asset resolution required to pin a Temurin JRE release and its
published checksum was not performed in this session. No Temurin version or
hash is recorded anywhere in this repository.

## Decision

- Detect an already-compatible Java at runtime and use it when the selected
  provider needs Java. Do not require it for Android CLI.
- When no compatible runtime exists, offer a private Temurin JRE, resolved from
  the Adoptium API to an exact release and published checksum, and persisted as
  a reviewed pin in a repository manifest.
- Do not reuse the Android CLI embedded runtime. It is not an executable
  runtime.
- Until a real classic-tools build is executed against a selected runtime,
  "Java >= 17" is not treated as an acceptance test.

## Consequences

The classic-tools path (needed for selected-image creation) is blocked on
resolving the Temurin pin and on installing `cmdline-tools` in an isolated
environment. Both are recorded as gaps in `docs/VALIDATION.md`.
