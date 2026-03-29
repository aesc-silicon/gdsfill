use clap::{Parser, Subcommand};
use std::path::PathBuf;

use gdsfill::{density, erase, fill, RunContext};

#[derive(Parser)]
#[command(name = "gdsfill", about = "Metal dummy fill for EDA layouts")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Erase dummy fill from a GDS layout
    Erase {
        /// PDK process name (e.g. ihp-sg13g2, ihp-sg13cmos5l)
        #[arg(long)]
        process: String,

        /// Fill config file; restricts erasing to the layers listed within
        #[arg(long)]
        config_file: Option<PathBuf>,

        /// Input GDS or GDS.GZ file (overwritten in place)
        gds_file: PathBuf,
    },

    /// Calculate metal density per layer and per tile
    Density {
        /// PDK process name (e.g. ihp-sg13g2, ihp-sg13cmos5l)
        #[arg(long)]
        process: String,

        /// Fill config file; restricts analysis to the layers listed within
        #[arg(long)]
        config_file: Option<PathBuf>,

        /// Write debug shapes to GDS (merged polygons: datatype 251)
        #[arg(long)]
        debug: bool,

        /// Input GDS or GDS.GZ file
        gds_file: PathBuf,
    },

    /// Add dummy fill to a GDS layout
    Fill {
        /// PDK process name (e.g. ihp-sg13g2, ihp-sg13cmos5l)
        #[arg(long)]
        process: String,

        /// Fill config file; overrides default density/deviation per layer
        #[arg(long)]
        config_file: Option<PathBuf>,

        /// Write debug shapes to GDS (keepout: datatype 250, merged metal: datatype 251)
        #[arg(long)]
        debug: bool,

        /// Don't merge metal dummy fill into input GDS file
        #[arg(long)]
        dry_run: bool,

        /// Input GDS or GDS.GZ file (overwritten in place)
        gds_file: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    eprintln!(
        "WARNING: gdsfill is experimental. Generated fill may produce DRC violations. \
         Always verify results before tape-out.\n"
    );

    let cli = Cli::parse();

    match cli.command {
        Commands::Erase { process, config_file, gds_file } => {
            let ctx = RunContext::new(&process, config_file.as_deref())?;
            erase::run(&gds_file, ctx)?;
        }
        Commands::Density { process, config_file, debug, gds_file } => {
            let ctx = RunContext::new(&process, config_file.as_deref())?;
            density::run(&gds_file, ctx, debug)?;
        }
        Commands::Fill { process, config_file, debug, dry_run, gds_file } => {
            let ctx = RunContext::new(&process, config_file.as_deref())?;
            fill::run(&gds_file, ctx, debug, dry_run)?;
        }
    }

    Ok(())
}
