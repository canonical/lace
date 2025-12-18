// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2025, Canonical Ltd.

use clap::Parser;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(version, about = "Build automation tasks for lace")]
enum Xtask {
    /// Generate man pages for all tools
    Mangen {
        /// Output directory for man pages (defaults to ./man)
        #[arg(short, long, default_value = "man")]
        output_dir: PathBuf,
    },
}

fn main() {
    let args = Xtask::parse();

    match args {
        Xtask::Mangen { output_dir } => {
            if let Err(e) = generate_man_pages(&output_dir) {
                eprintln!("Error generating man pages: {}", e);
                std::process::exit(1);
            }
        }
    }
}

fn generate_man_pages(output_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(output_dir)?;

    // Generate man page for pewrap
    let pewrap_cmd = <pewrap::cli::Args as clap::CommandFactory>::command();
    generate_man_page(&pewrap_cmd, "pewrap", output_dir)?;

    // Generate man page for collect-hwids
    let collect_hwids_cmd = <collect_hwids::cli::Args as clap::CommandFactory>::command();
    generate_man_page(&collect_hwids_cmd, "collect-hwids", output_dir)?;

    println!("Man pages generated in: {}", output_dir.display());
    Ok(())
}

fn generate_man_page(cmd: &clap::Command, name: &str, output_dir: &Path) -> std::io::Result<()> {
    let man = clap_mangen::Man::new(cmd.clone());
    let mut buffer = Vec::new();
    man.render(&mut buffer).map_err(std::io::Error::other)?;

    let man_path = output_dir.join(format!("{}.1", name));
    std::fs::write(&man_path, buffer)?;
    println!("  Generated: {}", man_path.display());

    Ok(())
}
