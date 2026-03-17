use clap::CommandFactory;
use clap_complete::{Shell, generate_to};
use clap_mangen::Man;
use std::fs;

include!("src/cli.rs");
include!("src/size_range.rs");

fn main() {
    let var = std::env::var_os("SHELL_COMPLETIONS_DIR").or_else(|| std::env::var_os("OUT_DIR"));
    let outdir = match var {
        None => return,
        Some(outdir) => outdir,
    };
    fs::create_dir_all(&outdir).unwrap();

    let mut cmd = Cli::command();

    for shell in [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::PowerShell] {
        generate_to(shell, &mut cmd, "pagers", &outdir).unwrap();
    }

    let man = Man::new(cmd.clone());
    let mut buffer: Vec<u8> = Default::default();
    man.render(&mut buffer).expect("Man page generation failed");

    let man_path = std::path::Path::new(&outdir).join("pagers.1");
    fs::write(man_path, buffer).expect("Failed to write main man page");

    for subcommand in cmd.get_subcommands() {
        let subcommand_name = subcommand.get_name();

        if subcommand_name == "help" {
            continue;
        }

        let man = Man::new(subcommand.clone());
        let mut buffer: Vec<u8> = Default::default();
        man.render(&mut buffer)
            .expect("Subcommand man page generation failed");

        let man_path = std::path::Path::new(&outdir).join(format!("pagers-{subcommand_name}.1"));
        fs::write(man_path, buffer).expect("Failed to write subcommand man page");
    }

    println!("cargo:rustc-cfg=pagers_normal_build");
}
