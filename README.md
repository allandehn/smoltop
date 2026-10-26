# smoltop

A small terminal UI for [smolvm](https://github.com/smol-machines/smolvm) microVMs. v0.1 talks to the `smolvm` binary on `PATH` (target CLI **v1.32.3**). It does not embed the hypervisor, open a socket of its own, or run `smolvm serve`.

The interface uses the terminal's ANSI colors (named colors, bold, dim, reverse). It does not set RGB, so it follows the Omarchy / Hyprland terminal theme.

## Install

`smolvm` 1.32.3 must already be on `PATH`. Then, from this repo:

```bash
cargo install --path .
smoltop
```

During development:

```bash
cargo test
cargo run
```

`cargo test` does not need `smolvm` and does not touch the desktop.

## Keybinds

| Key | Action |
| --- | --- |
| `j` / `k`, arrows | Select a machine |
| Enter | Shell: `smolvm machine exec --name NAME -it -- bash` (TUI suspends until it exits) |
| `s` | Start |
| `x` | Stop |
| `d`, then `y` or `n` | Delete. smoltop asks first, then passes `--force` so smolvm does not prompt again |
| `c` | Create. Form fields: name, image **or** a `.smolmachine` pack, network, DNS |
| `t` | Throwaway from a pack in the packs directory. Name is `tmp-1`, `tmp-2`, … |
| `p` | Pack a **stopped** machine: `smolvm pack create --from-vm NAME -o <packs dir>/NAME` (sidecar is `<name>.smolmachine`) |
| `y` | Copy a stopped machine: pack it, then `machine create --from` that sidecar. When network is enabled, the copy passes `--net` and `--dns` from the config, same as create and throwaway |
| `r` | Refresh now |
| `?` | Help |
| `q` | Quit |
| Ctrl-C | Quit from anywhere |
| Esc | Close help, or cancel a prompt |

The machine list polls `smolvm machine ls --json` about every 1.5 seconds. The selected machine's live CPU and memory come from `smolvm machine stats --name NAME --json` while it is running. If stats output is not the JSON shape, smoltop tries the human-readable stats text, and otherwise shows `stats unavailable` without dropping the list.

Every smolvm invocation starts with `--error-format json`. A failure is one line in the status bar (`CODE: message`).

In the create form: Tab / Shift-Tab moves fields, Left / Right changes source or network, Space toggles network, Enter creates, Esc cancels. DNS and the network switch are written back to the config file.

Names smoltop will pass through are `[A-Za-z0-9._-]`, must not start with `-`, must not be `.` or `..`, and are at most 128 bytes. smolvm itself is stricter (no dots, no trailing hyphen). If it rejects a name, the status line shows that error.

## Config

`~/.config/smoltop/config.toml`, or `$XDG_CONFIG_HOME/smoltop/config.toml` when that variable is set.

The file is optional. Defaults:

```toml
# dns is passed as --dns when network is true.
# packs_dir is scanned for *.smolmachine files.
dns = "100.64.0.7"
network = true
packs_dir = "~/smolvm"
```

- `network = true` sends `--net` and `--dns <dns>` on create, throwaway, and copy. `--dns` implies networking in smolvm; smoltop sends both. On `machine create --from`, smolvm ORs `--net` with the pack's own network bit, so turning the switch off omits the flags rather than forcing a pack offline. The default DNS `100.64.0.7` is what the host needs for a VM to resolve names.
- `packs_dir` is the directory `t`, `p`, and `y` use. Only direct `*.smolmachine` children with a safe file stem are listed. The path must not contain `..`.

Creating a machine from the form stores the DNS and network values you submitted. A config file that does not parse is left untouched and the defaults are used for that session.

## What smoltop will not pass

No shell (`sh -c`), no `sudo`, no `smolvm serve`, and none of `--ssh-agent`, `-v` / `--volume`, `--allow-system-mounts`, `--docker-socket`, `--mount-socket`, or `--secret-*`. Arguments are a `std::process::Command` vector, never a string handed to a shell.

## License

MIT. See [LICENSE](LICENSE).
