# DC Config Explorer

A desktop app for browsing the [Dragon City](https://dragoncitygame.com/) `game_config`: the large JSON file
the game downloads with its dragons, items, islands, chests, skills and events.

Built with [egui](https://www.egui.rs/) and [fastframe](https://fastframe.dev/).
Fan-made, not affiliated with SocialPoint.

## Why not just open it in a JSON editor?

You can, but the `game_config` is a very large file with sections holding thousands of entries,
and most of its text is translation keys (`tid_*`) instead of real names. A generic editor
struggles with that. DC Config Explorer understands the file:

| In a JSON editor | In DC Config Explorer |
| --- | --- |
| Opening and scrolling a huge file is slow, and some editors freeze | Each section opens on its own in a virtualized list that stays smooth with thousands of entries |
| You see `tid_dragon_name_1000` and have to look the text up somewhere else | The inspector shows the translated text next to each `tid_*` / `*_key` field, in the language you choose |
| Arrays of objects are walls of nested braces | Switch to a **table** view: one row per entry, one column per field |
| Entries read as `{ … }`, so you can't tell them apart | Each entry shows a hint from its `name`, `id`, `type`, `element` and similar fields |
| Raw sections mix ids, flags and keys in the game's internal layout | **Mapped** view: supported sections are restructured, validated and translated by [`dc-config-mapper`](https://crates.io/crates/dc-config-mapper) |
| Totals mean counting by hand or writing scripts | A **Statistics** dashboard counts dragons by rarity, element and category, and islands by status (active, upcoming, past) |
| The game's texts live in a separate localization file of ~38k keys | A **Localization** browser searches it (ignoring accents) and shows what changed between game versions |
| Getting the config means downloading it yourself | Load it from a file, a public URL or the game's authenticated endpoint |

## Features

### Overview
Lists every section of the config with its type, number of entries and whether it has a mapper.
You can filter sections by name in the sidebar.

### Section explorer
- **Raw** shows the section exactly as the game ships it.
- **Mapped** shows it restructured and translated. Available for `chests`, `items`, `skills`,
  `heroic_races` and the maze, tower, grid, fog and runner islands.
- **List or table** view, a filter by key or content, and a breadcrumb to jump back up.
- **Keyboard navigation**: ↑ / ↓ to move, Enter to open, Backspace to go up.
- **Inspector** for the selected entry: its JSON, the translations of its keys, plus buttons to copy
  its path (e.g. `items[12].name`) or its JSON, or export it to a file.

### Statistics
Totals and breakdowns from [`dc-statistics`](https://crates.io/crates/dc-statistics): dragons by
rarity, element and category, islands by status, items by group, and more.

### Localization
- Search the game's texts by key, by value or both. Accents are ignored, so `dragao` finds `dragão`.
- Export the current localization to a key-sorted JSON file, so two exports diff cleanly.
- Compare a saved file with the current version to see **new**, **edited** and **removed** texts.
  Handy for spotting upcoming content after a game update.

### Loading a config
- **Local file**: open it from the menu, drag and drop it onto the window, or pass it on the command line
  (`dc-config-explorer config.json`). Recent files are remembered.
- **Public URL** to a raw config.
- **Authenticated endpoint** (URL, user id and auth token), with a choice of language and platform.

Both the full game format (`{ "game_data": { "config": … } }`) and files holding only the sections object are accepted.

### Other
- Light and dark themes.
- Network requests, mapping and file writes run in the background, so the window never freezes.
- Settings are saved automatically. The auth token is saved only if you tick
  "Remember token", and goes into its own private file.
- Supports running as a portable copy on Windows.

## Building

Requires **Rust 1.98+** (needed by fastframe).

```sh
cargo run --release
cargo run --release -- path/to/config.json
```

## Tests

```sh
cargo test
# with a real config: runs every mapper and dc-statistics over it
DC_CONFIG_FIXTURE=/path/to/config.json cargo test --release -- --nocapture
# downloads the localization and tests export/compare (needs network)
DC_NETWORK_TESTS=1 cargo test --release -- --nocapture
```

## License

MIT
