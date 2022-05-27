use clap::FromArgMatches;

use super::*;

#[derive(Debug, Clone, Copy)]
pub struct Example {
    name: &'static str,
    bib: &'static str,
    json: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/example_files.rs"));

#[derive(Debug, Clone)]
pub struct ExampleName(String);

impl FromArgMatches for ExampleName {
    fn from_arg_matches(matches: &clap::ArgMatches) -> Result<Self, clap::Error> {
        Ok(ExampleName(matches.value_of("name").unwrap().to_string()))
    }

    fn update_from_arg_matches(&mut self, matches: &clap::ArgMatches) -> Result<(), clap::Error> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

impl Args for ExampleName {
    fn augment_args(cmd: clap::Command<'_>) -> clap::Command<'_> {
        cmd.arg(
            clap::Arg::new("name")
                .possible_values(EXAMPLES.iter().map(|e| e.name))
                .required(true)
                .help("Which example to show"),
        )
    }

    fn augment_args_for_update(cmd: clap::Command<'_>) -> clap::Command<'_> {
        Self::augment_args(cmd)
    }
}

#[derive(Args)]
pub struct ClArgs {
    #[clap(flatten)]
    name: ExampleName,

    /// Show Biblatex sample output as well
    #[clap(short = 'b')]
    show_bib: bool,
}

pub fn main(args: ClArgs) -> Result<()> {
    let e = EXAMPLES
        .iter()
        .find(|e| e.name == &args.name.0)
        .expect("example not found");

    println!("{}", e.json);
    if args.show_bib {
        println!("\n{}", e.bib);
    }

    Ok(())
}
