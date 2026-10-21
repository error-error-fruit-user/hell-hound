use clap::Parser;
use hell_hound::launcher::{Options, run};

fn main() {
    match run(Options::parse()) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("waw-zombies: {error:#}");
            std::process::exit(1);
        }
    }
}
