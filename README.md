# MiniGramps

A small Rust/egui desktop application for browsing and editing family trees.

## Run

```sh
cargo run --release
```

## Targets

MiniGramps keeps the core app shared and gates platform-specific integrations at
compile time:

- desktop: local filesystem, native file dialogs, window setup
- web/WASM: `eframe::WebRunner` entry point, currently starts with in-memory demo data
- Android: target path reserved; native runner, storage, and file/media pickers still need implementation

For a first web build, install `trunk` and run:

```sh
rustup target add wasm32-unknown-unknown
trunk serve
```

## Data

MiniGramps stores its own portable JSON files (`*.minigramps.json`). The default
library folder is the operating system's application-data folder. It can import
GEDCOM (`.ged`, `.gedcom`) and Gramps XML (`.gramps`, `.xml`) exports.
