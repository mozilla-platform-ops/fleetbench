use clap::{Parser, Subcommand, ValueEnum};

mod cpu;
mod inspect;

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
        Command::Cpu { mode, limit, iterations, threads, json } => {
            cpu::run(mode, limit, iterations, &threads, json)
        }
    };
    std::process::exit(exit_code);
}
