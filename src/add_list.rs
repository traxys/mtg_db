use color_eyre::eyre::Context;
use sha2::{Digest, Sha256};
use std::{cmp::Ordering, collections::HashSet, fs::OpenOptions, io::Write, path::PathBuf};
use structopt::StructOpt;

use crate::sqlite;

#[derive(StructOpt)]
pub struct Args {
    #[structopt(flatten)]
    database: sqlite::Args,
    list: PathBuf,
    #[structopt(long, short = "o")]
    save_on_error: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct CardInfo {
    promo: bool,
    set: String,
    name: String,
    id: String,
    uri: String,
    score: usize,
}

struct SortingCtx {
    chosen: HashSet<String>,
    chosen_set: HashSet<String>,
}

fn choose_correct_card(
    choice: &str,
    mut cards: Vec<CardInfo>,
    sorting_ctx: &mut SortingCtx,
) -> color_eyre::Result<CardInfo> {
    fn sort_cards(a: &CardInfo, b: &CardInfo) -> Ordering {
        a.score
            .cmp(&b.score)
            .then(a.name.cmp(&b.name).then(a.set.cmp(&b.set)))
    }

    fn sort_on_set(a: &CardInfo, b: &CardInfo, sorting_ctx: &SortingCtx) -> Ordering {
        match (
            sorting_ctx.chosen_set.contains(&a.set),
            sorting_ctx.chosen_set.contains(&b.set),
        ) {
            (true, true) | (false, false) => sort_cards(a, b),
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
        }
    }

    fn sort_on_id(a: &CardInfo, b: &CardInfo, sorting_ctx: &SortingCtx) -> Ordering {
        match (
            sorting_ctx.chosen.contains(&a.id),
            sorting_ctx.chosen.contains(&b.id),
        ) {
            (true, true) | (false, false) => sort_on_set(a, b, sorting_ctx),
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
        }
    }

    cards.sort_unstable_by(|a, b| sort_on_id(a, b, &sorting_ctx));

    println!("Choose match for {}:", choice);
    let mut iterator = cards.iter().enumerate();
    let mut remaining = cards.len();
    let mut show = true;
    loop {
        if show {
            for (idx, card) in iterator.by_ref().take(10) {
                println!(
                    "  - [{}] {} - {} [{}]({})",
                    idx + 1,
                    card.name,
                    card.set,
                    card.score,
                    card.uri
                );
                remaining -= 1;
            }
            show = false;
        }
        let text = if remaining == 0 {
            "Correct card ?"
        } else {
            "Correct card (0 for more choices)?"
        };
        let value: usize = promptly::prompt(text)?;
        if value > cards.len() {
            continue;
        } else if value == 0 {
            show = true;
        } else {
            let value = cards.into_iter().nth(value - 1).unwrap();
            sorting_ctx.chosen.insert(value.id.clone());
            sorting_ctx.chosen_set.insert(value.set.clone());
            return Ok(value);
        }
    }
}

impl Args {
    pub fn add_list(self) -> color_eyre::Result<()> {
        let card_list = std::fs::read_to_string(&self.list)?;

        let mut card_iter = card_list.lines().peekable();
        let card_uid: Vec<u8>;
        match card_iter.peek() {
            Some(l) if l.starts_with("uid=") => {
                let uid = card_iter.next().unwrap().strip_prefix("uid=").unwrap();
                card_uid = hex::decode(uid).wrap_err("could not decode uid")?;
            }
            _ => {
                let mut card_hasher = Sha256::new();
                card_hasher.update(&card_list);
                let card_hash = card_hasher.finalize();
                card_uid = card_hash.to_vec();
            }
        }

        match self.save_on_error {
            None => self.add_list_priv(card_iter, &card_uid, |_, _| Ok(())),
            Some(ref p) => {
                let mut out = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(p)?;
                writeln!(out, "uid={}", hex::encode(&card_uid))?;
                let mut was_writing = None;
                let list = self.add_list_priv(
                    card_iter.by_ref().inspect(|&line| was_writing = Some(line)),
                    &card_uid,
                    |id, foil| {
                        writeln!(out, "[id]{}{}", if foil { "[F]" } else { "" }, id)
                            .map(|_| ())
                            .map_err(Into::into)
                    },
                );
                if list.is_err() {
                    was_writing
                        .into_iter()
                        .chain(card_iter)
                        .try_for_each(|card| writeln!(out, "{}", card))?;
                }
                list
            }
        }
    }

    fn add_list_priv<'a, I, F>(
        &self,
        cards: I,
        card_uid: &[u8],
        mut added_card: F,
    ) -> color_eyre::Result<()>
    where
        I: Iterator<Item = &'a str>,
        F: FnMut(&str, bool) -> color_eyre::Result<()>,
    {
        let mut db = self.database.spellfix_connection()?;

        db.execute(
            r#"
            CREATE TABLE IF NOT EXISTS cards (
                id TEXT NOT NULL,
                foil BOOLEAN NOT NULL DEFAULT false,
                amount INTEGER NOT NULL,
                PRIMARY KEY (id, foil)
            )"#,
            [],
        )?;
        db.execute(
            "CREATE TABLE IF NOT EXISTS lists (hash BLOB PRIMARY KEY NOT NULL)",
            [],
        )?;

        let has_hash: usize = db.query_row(
            "SELECT COUNT(*) FROM lists WHERE hash = ?1",
            [card_uid],
            |row| row.get(0),
        )?;

        if has_hash > 0 {
            let cont: bool = promptly::prompt_default(
                "This list was already added, do you want to continue",
                false,
            )?;
            if cont == false {
                return Ok(());
            }
        }

        let tx = db.transaction()?;
        {
            let mut direct_match = tx.prepare(
                r#"
            SELECT 
                printed_name,
                id,
                uri,
                set_name,
                promo,
                0 as score 
            FROM 
                scryfall 
            WHERE 
                printed_name = ?1 
            UNION 
            SELECT 
                name,
                id,
                uri,
                set_name,
                promo,
                0 as score 
            FROM 
                scryfall 
            WHERE name = ?1
                AND printed_name IS NULL;
        "#,
            )?;

            let mut match_error = tx.prepare(
                r#"
            SELECT 
                IIF(printed_name IS NULL, name, printed_name) as nm,
                id,
                uri,
                set_name,
                promo,
                score 
            FROM 
                scryfall,
                card_names 
            WHERE 
                word MATCH ?1
                AND (
                    printed_name = word 
                    OR (
                        name = word 
                        AND printed_name IS NULL
                    )
                );
            "#,
            )?;

            let mut duo = tx.prepare(
                r#"
            SELECT 
                search.name,search.id,uri,set_name,promo,search.score
            FROM scryfall,
            (
                SELECT 
                    fn1.id as id,
                    (f1.score + f2.score)/2 as score,
                    fn1.name || ' // ' || fn2.name as name
                FROM 
                    face_names as f1,
                    scryfall_faces as fn1,
                    scryfall_faces as fn2,
                    face_names as f2 
                WHERE 
                    f1.word MATCH ?1
                    AND fn1.name = f1.word 
                    AND f2.word MATCH ?2
                    AND fn2.name = f2.word 
                    AND fn2.name != fn1.name 
                    AND fn1.id = fn2.id
                ORDER BY score
            ) as search 
            WHERE 
                search.id = scryfall.id;"#,
            )?;

            let mut sorting_ctx = SortingCtx {
                chosen: HashSet::new(),
                chosen_set: HashSet::new(),
            };

            for card in cards {
                let (foil, id) = match card.strip_prefix("[id]") {
                    Some(id) => match id.strip_prefix("[F]") {
                        None => (false, id.to_string()),
                        Some(id) => (true, id.to_string()),
                    },
                    None => {
                        let (foil, name) = match card.strip_prefix("[F]") {
                            None => (false, card),
                            Some(card) => (true, card),
                        };

                        let parse_row = |row: &rusqlite::Row| -> rusqlite::Result<_> {
                            Ok(CardInfo {
                                name: row.get(0)?,
                                id: row.get(1)?,
                                uri: row.get(2)?,
                                set: row.get(3)?,
                                promo: row.get(4)?,
                                score: row.get(5)?,
                            })
                        };

                        let names: Vec<CardInfo>;
                        if let Some(p) = name.find("//") {
                            println!("Handling double card {}", name);
                            let (first, second) = name.split_at(p);
                            let first = first.trim();
                            let second = (&second[2..]).trim();
                            names = duo
                                .query_map([first, second], &parse_row)?
                                .collect::<Result<_, _>>()?;
                        } else {
                            names = direct_match
                                .query_map([name], &parse_row)?
                                .collect::<Result<_, _>>()?;
                        }

                        let id = match names.len() {
                            0 => {
                                let corrections: Vec<CardInfo> = match_error
                                    .query_map([name], &parse_row)?
                                    .collect::<Result<_, _>>()?;

                                choose_correct_card(name, corrections, &mut sorting_ctx)?.id
                            }
                            1 => names.into_iter().next().unwrap().id,
                            _ => choose_correct_card(name, names, &mut sorting_ctx)?.id,
                        };
                        (foil, id)
                    }
                };

                added_card(&id, foil)?;

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

            tx.execute("INSERT OR IGNORE INTO lists (hash) VALUES (?1)", [card_uid])?;
        }
        tx.commit()?;

        Ok(())
    }
}
