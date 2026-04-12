use clap::Parser;

/// Ymir: a causal star-to-surface planet simulator grounded in real astronomical data.
#[derive(Parser)]
#[command(name = "ymir", version = "0.1.0", about)]
struct Cli {}

fn main() {
    let _cli = Cli::parse();
    println!("ymir v0.1.0");
}
