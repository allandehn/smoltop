# smoltop

A small terminal UI for [smolvm](https://github.com/smol-machines/smolvm) microVMs. v0.1 talks to the `smolvm` binary on `PATH` (target CLI **v1.32.3**). It does not embed the hypervisor, open a socket of its own, or run `smolvm serve`.

The interface uses the terminal's ANSI palette only: the named colors (`Color::Black` through `Color::White`, including the bright variants), plus bold, dim, and reverse. It does not set RGB or a 256-color index. Omarchy themes recolor those palette entries in place, so the screen follows the theme on dark and light backgrounds.

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

## Networking and DNS (VPN users)

`network` and `dns` are sent on create, throwaway, and copy (`--net --dns <dns>` when network is on). They exist because a microVM does not share the host's DNS path.

VPN clients that block DNS leaks drop queries to any resolver except their own. The host can still resolve names, because systemd-resolved answers on `127.0.0.53` and forwards through the tunnel. That address is the host's loopback stub. A VM cannot reach it, and a public resolver such as `1.1.1.1` or `8.8.8.8` is exactly what leak protection rejects. The VM has to be told the VPN's own DNS address.

This is not specific to one provider. It shows up with Mullvad, ProtonVPN, and IVPN, and with similar clients such as NordVPN and Windscribe when their leak protection is on. The address differs per client and sometimes per server. Look it up on the tunnel interface rather than hard-coding a guess:

```bash
resolvectl dns wg0-mullvad
# Link 12 (wg0-mullvad): 100.64.0.7
```

Use whichever interface the VPN created (`resolvectl dns` with no argument lists them). Then put that address in the config:

```toml
# ~/.config/smoltop/config.toml
dns = "100.64.0.7"
network = true
```

`100.64.0.7` is Mullvad's resolver on `wg0-mullvad`, and it is smoltop's default because that is the address this machine needs. Point `dns` at a different resolver when the VPN is a different one. The create form writes the value you submit back to this file. Turning `network` off omits both flags; it does not rewrite a pack to be offline, because smolvm ORs `--net` with the pack's own network bit.

## Theme changes

Omarchy switches themes by updating the current theme and reloading terminal configs. Alacritty, Ghostty, and Kitty then swap the ANSI palette on the live screen. smoltop's colors are palette indexes, so they pick up that swap. To cover terminals that only recolor newly drawn cells, smoltop also polls the theme path on each tick (a `stat` of the symlink target, inode, and mtime) and, when it changes, clears the terminal and redraws. Focus gained and resize do the same full redraw.

Checked against [basecamp/omarchy](https://github.com/basecamp/omarchy): `bin/omarchy-theme-set` replaces `$HOME/.local/state/omarchy/current/theme` (it is a directory, `rm` then `mv`, not a symlink in current Omarchy). `migrations/1781043107.sh` moved the older `$HOME/.config/omarchy/current` tree — whose `theme` entry was a symlink — to `~/.local/state/omarchy/current`. smoltop watches both of those paths. If neither exists, or `HOME` is unset, the watch is skipped and the TUI still runs.

Override the path when the layout is different. A missing file is fine; `..` is not.

```toml
theme_path = "~/.local/state/omarchy/current/theme"
```

## What smoltop will not pass

No shell (`sh -c`), no `sudo`, no `smolvm serve`, and none of `--ssh-agent`, `-v` / `--volume`, `--allow-system-mounts`, `--docker-socket`, `--mount-socket`, or `--secret-*`. Arguments are a `std::process::Command` vector, never a string handed to a shell.

## License

MIT. See [LICENSE](LICENSE).
