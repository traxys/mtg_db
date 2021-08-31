use rusqlite::Connection;
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt)]
pub struct Args {
    #[structopt(short, long, env = "SPELLFIX_EXT")]
    spellfix: PathBuf,
    #[structopt(short, long)]
    pub database: PathBuf,
}

impl Args {
    pub fn spellfix_connection(&self) -> Result<Connection, rusqlite::Error> {
        let connection = Connection::open(&self.database)?;
        connection.load_extension(&self.spellfix, None)?;
        Ok(connection)
    }
}
