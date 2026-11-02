# MyDigitalBuddy

This is the Rust target repository. The read-only Python source is external;
its fixed commit and source map are in docs/PROVENANCE.json and .reversa/context.

Keep the core independent of UI, native sensors and filesystem effects.
Preserve permission gates, persistent panic, per-pet isolation, finite-value
validation, atomic file replacement and the single-writer lock.

Run cargo fmt --check, cargo clippy --all-targets --all-features -- -D warnings,
cargo test --all-targets and cargo test --no-default-features --all-targets.
Use --locked in CI. Test native UI changes on the relevant OS; cross-checking
types is not evidence of runtime or visual parity.

Do not claim complete behavioral equivalence from the numeric fixtures.
Record intentional changes in docs/MIGRATION.md. Keep upstream credits and
GPL-3.0-or-later. Never commit memory, credentials, the Python executable,
target outputs or unlicensed media.
