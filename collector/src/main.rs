use clap::{Parser, Subcommand, ValueEnum};

mod cpu;
mod env;
mod inspect;
mod schema;
mod sieve;

#[derive(Parser)]
#[command(name = "fleetbench", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Collect host and CPU metadata without running a benchmark.
    Inspect {
        #[arg(long)]
        json: bool,
    },
    /// Run CPU benchmark workloads.
    Cpu {
        #[arg(long, value_enum, default_value_t = Mode::Normal)]
        mode: Mode,
        #[arg(long)]
        limit: Option<u64>,
        #[arg(long)]
        iterations: Option<u32>,
        #[arg(long, default_value = "auto")]
        threads: String,
        #[arg(long)]
        json: bool,
        /// Skip the brief warmup run before timed iterations.
        #[arg(long)]
        no_warmup: bool,
    },
}

#[derive(Copy, Clone, ValueEnum)]
enum Mode {
    Quick,
    Normal,
    Long,
}

fn main() {
    let cli = Cli::parse();
    let exit_code = match cli.command {
        Command::Inspect { json } => inspect::run(json),
        Command::Cpu { mode, limit, iterations, threads, json, no_warmup } => {
            cpu::run(mode, limit, iterations, &threads, json, !no_warmup)
        }
    };
    std::process::exit(exit_code);
}
