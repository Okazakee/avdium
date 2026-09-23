# Avdium repository agent instructions

These are repository-local operational constraints. They apply in addition to
the installed pi harness instructions and the user's global policy. Where they
conflict, the explicit user instruction and installed harness rules win.

## What this project is

Avdium is a native Android Virtual Device and SDK manager. Read `PRODUCT.md`
for scope and `docs/AVDIUM-BOOTSTRAP.md` for the evidence baseline. Do not
re-derive the whole research phase; re-check a primary source only when the
behavior it documents is relevant and may have changed.

## Hard rules

1. **Never mutate a real user SDK or AVD during development or tests.** The
   developer's own `~/Android/Sdk` and `~/.android/avd` are discovery fixtures
   only. Mutation tests use disposable AVDs with unique names and dedicated data
   directories, or the fake tools under `tests/fixtures/bin`.
2. **Never require sudo** for normal application behavior. Diagnostics may
   suggest a fix; Avdium does not perform privileged changes.
3. **Execute argument arrays, never a reconstructed shell string.** Shell
   escaping is only for a copyable command preview.
4. **Keep blocking work off the UI thread.** No subprocess, network request or
   unbounded filesystem scan may run inside an egui frame.
5. **No telemetry.** Avdium sends nothing about the user. Pass supported
   telemetry opt-outs to invoked tools. Do not claim the Android guest makes no
   network requests.
6. **No secrets in the repository.** Do not commit credentials, tokens, keys,
   `.env` contents or license text captures.
7. **Preserve notices.** Reused third-party code keeps its original license and
   notices. Record provenance in `docs/decisions/`.
8. **Do not publish.** No push, tag, package upload or release without explicit
   user authorization through the actual workflow. `publish = false` and the
   inactive release workflow are intentional.

## Identity consistency

Use `Avdium` and `avdium` consistently: window title, README, CLI help,
diagnostics, packaging and release filenames. The application id is
`dev.okazakee.Avdium`. Do not use placeholder names.

## Verification

Run the repository checks before declaring work done:

```sh
./scripts/check.sh
```

That script is the single definition of the check set. CI and the pre-push hook
call the same file, so do not restate the commands here.

Do not weaken lints, delete tests or remove assertions to obtain green checks.
Distinguish compiled, automated-tested, visually inspected and manually
exercised outcomes, and say which one a claim rests on.

## Documentation ownership

- `PRODUCT.md` owns product scope and the guardrails for product claims.
- `docs/ROADMAP.md` owns the ordered plan, the goals behind the ordering, and
  the milestone exit criteria.
- `CONTRIBUTING.md` owns checks and contribution workflow.
- `PACKAGING.md` owns packaging and release mechanics.
- `docs/STATUS.md` owns the short current-state summary.
- `docs/VALIDATION.md` owns executed validation evidence.
- `docs/decisions/` owns short architecture decision records.
- Source and tests are authoritative for implementation behavior.

Do not duplicate volatile implementation facts across documents. When the plan
changes, update `docs/ROADMAP.md` rather than scattering the intent across
commit messages.
