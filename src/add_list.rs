use std::{
    cmp::Ordering,
    collections::HashSet,
    fs::File,
    io::{BufRead, BufReader},
    path::PathBuf,
};
use structopt::StructOpt;

use crate::sqlite;

#[derive(StructOpt)]
pub struct Args {
    #[structopt(flatten)]
    database: sqlite::Args,
    list: PathBuf,
}

#[derive(Debug, Clone)]
struct CardInfo {
    promo: bool,
    set: String,
    name: String,
    id: String,
    uri: String,
}

struct SortingCtx<'a> {
    similar: Vec<String>,
    chosen: &'a mut HashSet<String>,
}

impl SortingCtx<'_> {
    fn idx(&self, name: &str) -> Option<usize> {
        self.similar
            .iter()
            .enumerate()
            .find(|(_, n)| n.as_str() == name)
            .map(|(idx, _)| idx)
    }
}

fn choose_correct_card(
    mut cards: Vec<CardInfo>,
    sorting_ctx: SortingCtx,
) -> color_eyre::Result<CardInfo> {
    cards.sort_unstable_by(|a, b| {
        if sorting_ctx.chosen.contains(&a.id) {
            if sorting_ctx.chosen.contains(&b.id) {
                return a.name.cmp(&b.name);
            } else {
                return Ordering::Less;
            }
        } else {
            match (sorting_ctx.idx(&a.name), sorting_ctx.idx(&b.name)) {
                (Some(a), Some(b)) => a.cmp(&b),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => a.name.cmp(&b.name),
            }
        }
    });

    println!("Multiple matches:");
    for (idx, card) in cards.iter().enumerate() {
        println!(
            "  - [{}] {} - {} ({})",
            idx + 1,
            card.name,
            card.set,
            card.uri
        );
    }
    loop {
        let value: usize = promptly::prompt("Correct card ?")?;
        if value == 0 || value > cards.len() {
            continue;
        }
        let value = cards.into_iter().nth(value - 1).unwrap();
        sorting_ctx.chosen.insert(value.id.clone());
        return Ok(value);
    }
}

impl Args {
    pub fn add_list(self) -> color_eyre::Result<()> {
        let mut db = self.database.spellfix_connection()?;

        let card_list = BufReader::new(File::open(&self.list)?);

        db.execute("CREATE TABLE IF NOT EXISTS cards (id TEXT NOT NULL, foil BOOLEAN NOT NULL DEFAULT false, amount INTEGER NOT NULL, PRIMARY KEY (id, foil))", [])?;

        let tx = db.transaction()?;

        let mut printed = tx.prepare(
            "SELECT printed_name,id,uri,set_name,promo FROM scryfall WHERE printed_name = ?1",
        )?;
        let mut eng = tx.prepare(
            "SELECT name,id,uri,set_name,promo  FROM scryfall WHERE name = ?1 AND printed_name IS NULL",
        )?;

        let mut printed_error = tx.prepare("SELECT printed_name,id,uri,set_name,promo FROM scryfall WHERE printed_name IN (SELECT word FROM card_names WHERE word MATCH ?1);")?;
        let mut eng_error = tx.prepare("SELECT name,id,uri,set_name,promo FROM scryfall WHERE name IN (SELECT word FROM card_names WHERE word MATCH ?1);")?;
        let mut similar = tx.prepare("SELECT word FROM card_names WHERE word MATCH ?1")?;

        let mut chosen = HashSet::new();

        for card in card_list.lines() {
            let card = card?;
            let (foil, name) = match card.strip_prefix("[F]") {
                None => (false, card.as_str()),
                Some(card) => (true, card),
            };

            let parse_row = |row: &rusqlite::Row| -> rusqlite::Result<_> {
                Ok(CardInfo {
                    name: row.get(0)?,
                    id: row.get(1)?,
                    uri: row.get(2)?,
                    set: row.get(3)?,
                    promo: row.get(4)?,
                })
            };

            let names: Vec<CardInfo> = printed
                .query_map([name], &parse_row)?
                .chain(eng.query_map([name], &parse_row)?)
                .collect::<Result<_, _>>()?;

            let id = match names.len() {
                0 => {
                    println!("{} was not found, possible choices:", name);
                    let corrections: Vec<CardInfo> = printed_error
                        .query_map([name], &parse_row)?
                        .chain(eng_error.query_map([name], &parse_row)?)
                        .collect::<Result<_, _>>()?;

                    let similar = similar
                        .query_map([name], |row| row.get(0))?
                        .collect::<Result<_, _>>()?;
                    choose_correct_card(
                        corrections,
                        SortingCtx {
                            similar,
                            chosen: &mut chosen,
                        },
                    )?
                    .id
                }
                1 => names.into_iter().next().unwrap().id,
                _ => {
                    choose_correct_card(
                        names,
                        SortingCtx {
                            similar: Vec::new(),
                            chosen: &mut chosen,
                        },
                    )?
                    .id
                }
            };

            let is_present: usize = tx.query_row(
                "SELECT COUNT(*) FROM cards WHERE id = ?1 AND foil = ?2",
                rusqlite::params![&id, foil],
                |row| row.get(0),
            )?;
            if is_present == 0 {
                tx.execute(
                    "INSERT INTO cards (id, foil, amount) VALUES (?1, ?2, 1)",
                    rusqlite::params![&id, foil],
                )?;
            } else {
                tx.execute(
                    "UPDATE cards SET amount = amount + 1 WHERE id = ?1 AND foil = ?2",
                    rusqlite::params![&id, foil],
                )?;
            }
        }

        drop(printed);
        drop(eng);
        drop(printed_error);
        drop(eng_error);
        drop(similar);
        tx.commit()?;

        Ok(())
    }
}
