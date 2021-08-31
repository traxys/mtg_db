# MTG DB

This tool can generate and manipulate a sqlite database of all your MTG collection.

## Building

You need to compile the `spellfix` sqlite extension using `build_spellfix.sh`. You either can pass the generated `.so` as an argument through each invocation that needs it or set the `SPELLFIX_EXT` to it.

To build the binary you can just run `cargo build --release`.

## Usage

The first requirement is a scryfall [all cards dump](https://scryfall.com/docs/api/bulk-data). You then need to process the dump using `mtg_db jsonl` to create a `jsonl` file.

You can then pass this file to `mtg_db dump` to create/update the database with all mtg cards.

Finally you can add some of your cards using `mtg_db add-list` that takes a file with one card per line
