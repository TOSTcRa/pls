mod commands;
mod network;
mod types;
mod utils;

use std::env;

pub const REPO_URL: &str = "https://tostcra.github.io/aura-repo";
pub const CACHE_DIR: &str = "/var/cache/pls";
pub const PACKAGES_DIR: &str = "/var/lib/pls/packages";
pub const DB_DIR: &str = "/var/lib/pls/db";
pub const ROOT: &str = "/";

fn print_help() {
    println!("pls - package manager that doesn't mess with ya");
    println!();
    println!("usage: pls <command> [args]");
    println!();
    println!("commands:");
    println!("  install <pkg>     install a package (name, path, or url)");
    println!("  remove <pkg>      remove a package");
    println!("  info <pkg>        show package info");
    println!("  list              list installed packages");
    println!("  update            update all installed packages");
    println!("  add <path>        create package from project");
    println!("    --draft         use debug build instead of release");
    println!("    --output <dir>  output to custom directory");
    println!("  repo update       update index.json from packages/");
    println!("  bundle <name>     install a bundle (gaming, dev-rust, etc)");
    println!();
    println!("supported projects:");
    println!("  Rust      Cargo.toml");
    println!("  C/C++     CMakeLists.txt, meson.build");
    println!("  Any       pls.toml (manual config)");
    println!();
    println!("examples:");
    println!("  pls install yplay");
    println!("  pls install https://example.com/app.deb");
    println!("  pls add . --output ~/my-repo/packages/");
    println!("  pls repo update");
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_help();
        return;
    }

    let command = &args[1];

    let result = match command.as_str() {
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        "install" => {
            if args.len() < 3 {
                Err("install what? give me a package name".to_string())
            } else {
                commands::cmd_install(&args[2]).await
            }
        }
        "remove" | "rm" => {
            if args.len() < 3 {
                Err("remove what?".to_string())
            } else {
                commands::cmd_remove(&args[2])
            }
        }
        "info" => {
            if args.len() < 3 {
                Err("info about what?".to_string())
            } else {
                commands::cmd_info(&args[2])
            }
        }
        "list" | "ls" => commands::cmd_list(),
        "update" => commands::cmd_update().await,
        "add" => {
            let path = if args.len() >= 3 && !args[2].starts_with('-') {
                &args[2]
            } else {
                "."
            };
            let is_draft = args.iter().any(|a| a == "--draft");
            let output_dir = args
                .iter()
                .position(|a| a == "--output" || a == "-o")
                .and_then(|i| args.get(i + 1))
                .map(|s| s.as_str());

            commands::cmd_add(path, is_draft, output_dir)
        }
        "repo" => {
            if args.len() < 3 {
                Err("repo what? try 'pls repo update'".to_string())
            } else if args[2] == "update" {
                commands::cmd_repo_update()
            } else {
                Err(format!("unknown repo command: {}", args[2]))
            }
        }
        "bundle" => {
            if args.len() < 3 {
                Err("bundle what? try 'pls bundle gaming'".to_string())
            } else {
                commands::cmd_bundle(&args[2]).await
            }
        }
        _ => Err(format!("nah '{}' is not a thing, try 'pls help'", command)),
    };

    if let Err(e) = result {
        eprintln!("nah bro: {}", e);
        std::process::exit(1);
    }
}
