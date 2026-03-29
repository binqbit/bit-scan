mod check;
mod scan_v1;
mod scan_v2;
mod scan_v3;
mod scan_v4;
pub mod utils;

use std::num::NonZeroUsize;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "bit-scan", version, about = "Bitcoin puzzle wallet scanner")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Search for a private key matching the provided wallet
    Scan {
        /// Selects the scanning engine
        #[arg(value_enum, short = 'v', long = "version", default_value_t = ScanVersion::V2)]
        version: ScanVersion,
        /// Prints rolling throughput statistics once per second
        #[arg(long)]
        stats: bool,
        /// Target wallet address or puzzle number
        address: String,
        /// Number of worker threads (required for version v4)
        #[arg(long, value_name = "COUNT")]
        threads: Option<NonZeroUsize>,
    },
    /// Validate that a private key matches a wallet address
    Check {
        /// Target wallet address or puzzle number
        address: String,
        /// Private key encoded as hex (optionally 0x-prefixed)
        private_key: String,
    },
}

#[derive(Copy, Clone, Debug, ValueEnum, PartialEq, Eq)]
enum ScanVersion {
    V1,
    V2,
    V3,
    V4,
}

fn main() {
    let cli = Cli::parse();
    utils::warm_puzzle_cache();

    match cli.command {
        Commands::Scan {
            version,
            stats,
            address,
            threads,
        } => {
            let resolved = match utils::resolve_target(&address) {
                Ok(res) => res,
                Err(err) => {
                    eprintln!("{err}");
                    std::process::exit(3);
                }
            };

            let effective_bits = resolved.suggested_bits.unwrap_or_else(|| {
                eprintln!(
                    "Unable to infer bit length for target \"{address}\". \
                     Please add it to config/puzzle_addresses.csv."
                );
                std::process::exit(4);
            });

            if version != ScanVersion::V4 && threads.is_some() {
                eprintln!("--threads is only supported when --version v4 is selected");
                std::process::exit(5);
            }

            match version {
                ScanVersion::V1 => scan_v1::scan(&resolved.address, effective_bits, stats),
                ScanVersion::V2 => scan_v2::scan(&resolved.address, effective_bits, stats),
                ScanVersion::V3 => scan_v3::scan(&resolved.address, effective_bits, stats),
                ScanVersion::V4 => {
                    let thread_count = threads.map(NonZeroUsize::get).unwrap_or_else(|| {
                        eprintln!("--threads <COUNT> is required when --version v4 is selected");
                        std::process::exit(6);
                    });
                    scan_v4::scan(&resolved.address, effective_bits, stats, thread_count);
                }
            }
        }
        Commands::Check {
            address,
            private_key,
        } => {
            let resolved = match utils::resolve_target(&address) {
                Ok(res) => res,
                Err(err) => {
                    eprintln!("{err}");
                    std::process::exit(3);
                }
            };
            match check::check(&resolved.address, &private_key) {
                Ok(true) => {}
                Ok(false) => std::process::exit(1),
                Err(err) => {
                    eprintln!("{err}");
                    std::process::exit(2);
                }
            }
        }
    }
}
