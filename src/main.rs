use structopt::StructOpt;

mod add_list;
mod dump;
mod jsonl;
mod sqlite;

#[derive(StructOpt)]
struct Args {
    #[structopt(subcommand)]
    commands: Commands,
}

#[derive(StructOpt)]
enum Commands {
    AddList(add_list::Args),
    Dump(dump::Args),
    Jsonl(jsonl::Args),
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = Args::from_args();
    match args.commands {
        Commands::AddList(sub_args) => sub_args.add_list(),
        Commands::Dump(sub_args) => sub_args.dump_scryfall(),
        Commands::Jsonl(sub_args) => sub_args.convert_jsonl(),
    }
}
