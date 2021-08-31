use indicatif::ProgressIterator;
use serde_json::Value;
use std::{
    fs::{File, OpenOptions},
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
};
use structopt::StructOpt;

#[derive(StructOpt)]
pub struct Args {
    input: PathBuf,
    output: PathBuf,
}

impl Args {
    pub fn convert_jsonl(self) -> color_eyre::Result<()> {
        println!("Parsing input:");
        let input = BufReader::new(File::open(self.input)?);
        let input: Value = serde_json::from_reader(input)?;

        let mut output = BufWriter::new(
            OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(self.output)?,
        );

        println!("Creating jsonl file:");
        match input {
            Value::Array(arr) => {
                writeln!(output, "{}", arr.len())?;
                for elem in arr.iter().progress() {
                    serde_json::to_writer(&mut output, &elem)?;
                    writeln!(output)?;
                }
            }
            Value::Object(obj) => {
                writeln!(output, "{}", obj.len())?;
                for (key, value) in obj.iter().progress() {
                    serde_json::to_writer(&mut output, &serde_json::json!({ key: value }))?;
                    writeln!(output)?;
                }
            }
            single => {
                writeln!(output, "1")?;
                serde_json::to_writer(&mut output, &single)?;
                writeln!(output)?;
            }
        }

        Ok(())
    }
}
