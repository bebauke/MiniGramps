# MiniGramps

A small Rust/egui desktop application for browsing and editing family trees.

## Run

```sh
cargo run --release
```

## Data

MiniGramps stores its own portable JSON files (`*.minigramps.json`). The default
library folder is the operating system's application-data folder. It can import
GEDCOM (`.ged`, `.gedcom`) and Gramps XML (`.gramps`, `.xml`) exports.
