# TODO

## Done (v0.1)

- Theme-following ANSI colors and highlights.
- Live Omarchy theme switching.
- VPN/DNS README section (Mullvad, ProtonVPN, IVPN and similar).
- ` · ` key-hint separators.
- Copy passes net/DNS.

## Next up

- Test against a real smolvm 1.32.3 and a real Omarchy theme switch.
- First release v0.1.0. `scripts/release.sh` never pushes `main`.
  - On up-to-date main: `scripts/release.sh 0.1.0 --dry-run`, then `scripts/release.sh 0.1.0`. That pushes branch `release/0.1.0` (Cargo.toml and Cargo.lock only) for a pull request.
  - After that pull request is merged, on up-to-date main: `scripts/release.sh 0.1.0 --publish --dry-run`, then `scripts/release.sh 0.1.0 --publish`. Publish builds the x86_64 Linux tarball and `.sha256`, tags `v0.1.0`, pushes only the tag, and attaches both files to the GitHub release.

## Ideas

Not scheduled. Small ideas for after v0.1:

- Pause and resume a machine from the list.
- Tail the guest console without leaving the TUI.
- Remember the selected machine across launches.
- Show which theme path is being watched, when one is present.
