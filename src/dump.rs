use color_eyre::eyre::WrapErr;
use indicatif::{ParallelProgressIterator, ProgressIterator};
use rayon::prelude::*;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::PathBuf,
};
use structopt::StructOpt;

use crate::sqlite;

#[derive(Serialize, Deserialize, Debug)]
struct Card {
    id: String,
    scryfall_uri: String,
    name: String,
    printed_name: Option<String>,
    set_name: String,
    promo: bool,
    prices: Price,
    variation: bool,
}

#[derive(Serialize, Deserialize, Debug)]
struct Price {
    eur: Option<String>,
    eur_foil: Option<String>,
}

#[derive(StructOpt)]
pub struct Args {
    #[structopt(flatten)]
    database: sqlite::Args,
    scryfall_jsonl: PathBuf,
}

impl Args {
    pub fn dump_scryfall(self) -> color_eyre::Result<()> {
        let attach_str = format!("ATTACH '{}' as sc; BEGIN; INSERT INTO sc.scryfall SELECT * FROM scryfall; COMMIT; DETACH sc;", self.database.database.to_string_lossy());

        let sc = &self.database.spellfix_connection()?;
        sc.execute("DROP TABLE IF EXISTS scryfall;", [])?;
        let table = r#"CREATE TABLE scryfall (
            id TEXT PRIMARY KEY NOT NULL, 
            name TEXT NOT NULL, 
            printed_name TEXT, 
            eur TEXT, 
            eur_foil TEXT, 
            uri TEXT NOT NULL,
            set_name TEXT NOT NULL,
            promo BOOLEAN NOT NULL,
            variation BOOLEAN NOT NULL);"#;

        sc.execute(&table, [])?;

        println!("Creating scryfall databases:");
        let mut input = BufReader::new(File::open(&self.scryfall_jsonl)?).lines();
        let count = input
            .next()
            .ok_or(color_eyre::eyre::eyre!("input file is empty"))??;

        let parts: Vec<Connection> = input.enumerate()
        .par_bridge()
        .progress_count(count.parse()?)
        .map(|(idx, line)| -> color_eyre::Result<Card> {serde_json::from_str(&line?).wrap_err_with(|| format!("Error at line {}", idx))})
        .try_fold(
            || {
                let con = Connection::open_in_memory().expect("could not open in memory");
                con.execute(&table, []).expect("could not create schema");
                con
            },
            |con, card| -> color_eyre::Result<_> {
                let card = card?;
                let name = card.name.to_lowercase();
                let printed_name =
                    card.printed_name
                        .as_deref()
                        .map(deunicode::deunicode)
                        .map(|mut s| {
                            s.make_ascii_lowercase();
                            s
                        });

                con.execute(
                    r#"INSERT OR IGNORE INTO scryfall (id, name, printed_name, eur, eur_foil, uri, set_name, promo, variation) 
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"#,
                    rusqlite::params![card.id, name, printed_name, card.prices.eur, card.prices.eur_foil, card.scryfall_uri, card.set_name, card.promo, card.variation],
                )?;
                Ok(con)
            },
        ).collect::<color_eyre::Result<_>>()?;

        println!("Merging databases:");
        parts
            .iter()
            .progress()
            .try_for_each(|connection| connection.execute_batch(&attach_str))?;

        println!("Creating vocabulary:");
        sc.execute_batch(r#"
                         DROP TABLE IF EXISTS card_names;
                         CREATE VIRTUAL TABLE card_names USING spellfix1;
                         INSERT INTO card_names(word) SELECT DISTINCT printed_name FROM scryfall WHERE printed_name IS NOT NULL;
                         INSERT INTO card_names(word) SELECT DISTINCT name FROM scryfall;
                         "#)?;

        Ok(())
    }
}
