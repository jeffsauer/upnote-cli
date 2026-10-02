# upnote-cli

Query your local [UpNote](https://upnote.com) database from the terminal and render notes as formatted markdown — including inline images — without opening the app.

`upnote-cli` reads the SQLite database and image cache that the UpNote desktop application maintains on your machine, then renders everything with [glamour](https://crates.io/crates/charmed-glamour) (the same markdown renderer used by Charm's `glow`).

## How it works

- **Data source:** `~/.config/UpNote/upnote.sqlite3` (opened **read-only**) and the image cache at `~/.config/UpNote/images/`.
- **Rendering:** note HTML is converted to markdown and rendered to your terminal. Lists, tables, and code blocks are all formatted; code blocks get syntax highlighting.
- **Images:** supported terminals (kitty and compatible) get inline images via the [kitty graphics protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/). See [Images](#images) for details.

The tool never writes to your UpNote data. It is a read-only viewer.

## Prerequisites

- The [Rust toolchain](https://rustup.rs) (`cargo`).
- The [UpNote desktop app](https://upnote.com) installed and signed in (so the local database and image cache exist).
- To see inline images, a terminal that speaks the kitty graphics protocol — **kitty**, **WezTerm**, or **Ghostty**. In any other terminal, images are shown as text placeholders (the rest of the note still renders normally).

## Building

```sh
cargo build --release
# binary at: target/release/upnote-cli
```

## Usage

```
upnote-cli [OPTIONS] <COMMAND>
```

### Commands

| Command | Description |
|---|---|
| `search <query...>` | Search notes by title, summary, text, or content. One match prints the note; several prints a numbered, alphabetical list you can pick from. |
| `note <id-or-title>` | Show a single note by id or (partial) title. |
| `notebooks` | Print the full notebook tree with per-notebook note counts. |
| `notebook <name-or-id> [--recursive]` | List notes in a notebook (add `--recursive` to include sub-notebooks). |
| `tags` | List all tags with note counts. |
| `tag <name>` | List notes in a tag (leading `#` optional). |
| `list [MODE]` | List notes by category: `all` (default), `trash`, `pinned`, `bookmarked`, `templates`. |
| `stats` | Database statistics (counts, cached image count, DB size). |

### Global options

| Option | Description |
|---|---|
| `--db <PATH>` | Path to `upnote.sqlite3` (default `~/.config/UpNote/upnote.sqlite3`). |
| `--theme <THEME>` | `dark` (default), `light`, `pink`, `dracula`, `tokyo-night`, `ascii`. |
| `--plain` | Plain output: no color, no inline images. |
| `--no-images` | Disable inline images. |
| `--width <COLS>` | Terminal width for wrapping (default: detected). |
| `--limit <N>` | Max notes shown in listings (default `50`). |

### Examples

```sh
upnote-cli search "fiber optic"
upnote-cli note "Quadrafire Pellet Stove"
upnote-cli notebook "Rover Builds" --recursive
upnote-cli tag "#mechanics"
upnote-cli list pinned
upnote-cli --theme pink --width 100 note "My Note"
upnote-cli --plain stats        # for piping / non-tty use
```

## Images

When run in a compatible terminal, cached images are embedded inline.

- **Cached images** (already in `~/.config/UpNote/images/`) render immediately.
- **Uncached images** are shown as a placeholder, e.g. `[image not cached] photo.png — open this note in the UpNote app to download it`. The UpNote app downloads images from its own servers when you view a note, so opening the note once in the app makes its images appear here afterwards.
- **Attachments backup fallback:** if you have enabled *Backup attachments* in the UpNote preferences (`SHOULD_BACKUP_ATTACHMENTS` in `~/.config/UpNote/renderer.json`), `upnote-cli` also looks in the newest `~/.config/UpNote/UpNote Backup/*/files` directory. Backup folders are always treated as read-only.

In terminals that don't support the kitty protocol, or with `--plain` / `--no-images`, images appear as dim text placeholders instead.

## Notes

- This is a personal productivity tool that operates on your local, already-downloaded data. It sends nothing anywhere and makes no network requests.
- The database is opened read-only; nothing in your UpNote data is modified.

## License

[MIT](LICENSE)
