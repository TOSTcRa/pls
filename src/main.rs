use std::env;
use std::fs;
use std::fs::File;
use std::io;
use std::path::Path;
use std::process::Command;
use tar::Archive;

const PACKAGES_DIR: &str = "/var/lib/pls/packages";
const DB_DIR: &str = "/var/lib/pls/db";
const ROOT: &str = "/";

struct PackageInfo {
    name: String,
    version: String,
    depend: Vec<String>,
}

impl PackageInfo {
    fn parse_info(content: &str) -> Self {
        let mut name = String::new();
        let mut version = String::new();
        let mut depend = Vec::new();

        for line in content.lines() {
            let line = line.trim();
            if let Some((key, value)) = line.split_once(" = ") {
                match key {
                    "name" => name = value.to_string(),
                    "version" => version = value.to_string(),
                    "depend" => depend.push(value.to_string()),
                    _ => {}
                }
            }
        }
        Self {
            name,
            version,
            depend,
        }
    }

    fn from_file(path: &str) -> io::Result<Self> {
        let content = fs::read_to_string(path)?;
        Ok(Self::parse_info(&content))
    }

    fn parse_cargo_toml(content: &str) -> Self {
        let mut name = String::new();
        let mut version = String::new();
        let mut depend = Vec::new();
        let mut section = String::new();

        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                section = line.trim_matches(|c| c == '[' || c == ']').to_string();
            } else if section == "package" {
                if let Some((key, value)) = line.split_once(" = ") {
                    match key {
                        "name" => name = value.trim_matches('"').to_string(),
                        "version" => version = value.trim_matches('"').to_string(),
                        _ => {}
                    }
                }
            } else if section == "dependencies" {
                if let Some((dep_name, _)) = line.split_once(" = ") {
                    depend.push(dep_name.trim().to_string());
                }
            }
        }
        Self {
            name,
            version,
            depend,
        }
    }
}

fn extract_package(archive_path: &str, dest: &str) -> io::Result<()> {
    let _ = fs::remove_dir_all(dest);
    fs::create_dir_all(dest)?;

    let file = File::open(archive_path)?;
    let decoder = zstd::stream::Decoder::new(file)?;
    let mut archive = Archive::new(decoder);
    archive.unpack(dest)?;
    Ok(())
}

fn create_package(source_dir: &str, output_path: &str) -> io::Result<()> {
    let file = File::create(output_path)?;
    let encoder = zstd::stream::Encoder::new(file, 3)?;
    let mut tar = tar::Builder::new(encoder);
    tar.append_dir_all(".", source_dir)?;
    let encoder = tar.into_inner()?;
    encoder.finish()?;
    Ok(())
}

fn resolve_package_path(input: &str) -> Option<String> {
    if input.contains('/') || input.ends_with(".pls") {
        if Path::new(input).exists() {
            return Some(input.to_string());
        }
        return None;
    }

    let repo_path = format!("{}/{}.pls", PACKAGES_DIR, input);
    if Path::new(&repo_path).exists() {
        return Some(repo_path);
    }
    None
}

fn is_installed(name: &str) -> bool {
    Path::new(&format!("{}/{}", DB_DIR, name)).exists()
}

fn print_help() {
    println!("pls - package manager that doesn't mass with ya");
    println!();
    println!("usage: pls <command> [args]");
    println!();
    println!("commands:");
    println!("  install <pkg>     install a package (name or path)");
    println!("  remove <pkg>      remove a package");
    println!("  info <pkg>        show package info");
    println!("  list              list installed packages");
    println!("  add <path>        create package from project");
    println!("    --draft         use debug build instead of release");
    println!();
    println!("examples:");
    println!("  pls install yplay");
    println!("  pls add . --draft");
}

fn cmd_install(package_input: &str) -> Result<(), String> {
    let package_path = resolve_package_path(package_input)
        .ok_or_else(|| format!("sorry bro, couldn't find '{}' anywhere", package_input))?;

    let temp_dir = "/tmp/pls-extract";

    extract_package(&package_path, temp_dir)
        .map_err(|e| format!("couldn't unpack that thing: {}", e))?;

    let pkg = PackageInfo::from_file(&format!("{}/info", temp_dir))
        .map_err(|_| "package seems broken, no info file found")?;

    if is_installed(&pkg.name) {
        println!("yo {} is already installed, reinstalling...", pkg.name);
    }

    fs::create_dir_all(format!("{}/usr/bin", ROOT))
        .map_err(|e| format!("couldn't create bin dir: {}", e))?;

    let status = Command::new("cp")
        .args([
            "-r",
            &format!("{}/bin/.", temp_dir),
            &format!("{}/usr/bin/", ROOT),
        ])
        .status()
        .map_err(|e| format!("copy failed: {}", e))?;

    if !status.success() {
        return Err("couldn't copy files, check permissions maybe?".to_string());
    }

    let db_path = format!("{}/{}", DB_DIR, pkg.name);
    fs::create_dir_all(&db_path).map_err(|e| format!("couldn't create db entry: {}", e))?;
    fs::copy(format!("{}/info", temp_dir), format!("{}/info", db_path))
        .map_err(|e| format!("couldn't save package info: {}", e))?;

    let _ = fs::remove_dir_all(temp_dir);

    println!("got ya! {} v{} installed", pkg.name, pkg.version);
    Ok(())
}

fn cmd_remove(package_name: &str) -> Result<(), String> {
    if !is_installed(package_name) {
        return Err(format!("'{}' isn't even installed bro", package_name));
    }

    let bin_path = format!("{}/usr/bin/{}", ROOT, package_name);
    if Path::new(&bin_path).exists() {
        fs::remove_file(&bin_path).map_err(|e| format!("couldn't delete binary: {}", e))?;
    }

    let db_path = format!("{}/{}", DB_DIR, package_name);
    fs::remove_dir_all(&db_path).map_err(|e| format!("couldn't remove from db: {}", e))?;

    println!("gone! {} has been removed", package_name);
    Ok(())
}

fn cmd_info(package_input: &str) -> Result<(), String> {
    let package_path = resolve_package_path(package_input)
        .ok_or_else(|| format!("couldn't find '{}'", package_input))?;

    let temp_dir = "/tmp/pls-info";
    extract_package(&package_path, temp_dir).map_err(|e| format!("couldn't unpack: {}", e))?;

    let pkg = PackageInfo::from_file(&format!("{}/info", temp_dir))
        .map_err(|_| "no info file in package")?;

    println!("name: {}", pkg.name);
    println!("version: {}", pkg.version);
    if !pkg.depend.is_empty() {
        println!("depends: {}", pkg.depend.join(", "));
    }

    let _ = fs::remove_dir_all(temp_dir);
    Ok(())
}

fn cmd_list() -> Result<(), String> {
    if !Path::new(DB_DIR).exists() {
        println!("nothing installed yet");
        return Ok(());
    }

    let entries = fs::read_dir(DB_DIR).map_err(|_| "couldn't read package database")?;

    let mut count = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let info_path = format!("{}/{}/info", DB_DIR, name.to_string_lossy());

        if let Ok(pkg) = PackageInfo::from_file(&info_path) {
            println!("{} v{}", pkg.name, pkg.version);
            count += 1;
        }
    }

    if count == 0 {
        println!("nothing installed yet");
    } else {
        println!("\n{} package(s) installed", count);
    }
    Ok(())
}

fn cmd_add(project_path: &str, is_draft: bool) -> Result<(), String> {
    let cargo_path = format!("{}/Cargo.toml", project_path);

    if !Path::new(&cargo_path).exists() {
        return Err("no Cargo.toml found, is this a rust project?".to_string());
    }

    let cargo_content = fs::read_to_string(&cargo_path).map_err(|_| "couldn't read Cargo.toml")?;
    let pkg = PackageInfo::parse_cargo_toml(&cargo_content);

    if pkg.name.is_empty() {
        return Err("couldn't parse project name from Cargo.toml".to_string());
    }

    let (build_type, binary_path) = if is_draft {
        (
            "debug",
            format!("{}/target/debug/{}", project_path, pkg.name),
        )
    } else {
        (
            "release",
            format!("{}/target/release/{}", project_path, pkg.name),
        )
    };

    if !Path::new(&binary_path).exists() {
        println!("building {} {}...", build_type, pkg.name);

        let mut args = vec!["build"];
        if !is_draft {
            args.push("--release");
        }

        let status = Command::new("cargo")
            .args(&args)
            .current_dir(project_path)
            .status()
            .map_err(|e| format!("cargo failed: {}", e))?;

        if !status.success() {
            return Err("build failed, fix ur code first".to_string());
        }
    }

    if !Path::new(&binary_path).exists() {
        return Err(format!("binary not found at {}", binary_path));
    }

    let build_dir = "/tmp/pls-build";
    let _ = fs::remove_dir_all(build_dir);
    fs::create_dir_all(format!("{}/bin", build_dir))
        .map_err(|_| "couldn't create build directory")?;

    fs::copy(&binary_path, format!("{}/bin/{}", build_dir, pkg.name))
        .map_err(|_| "couldn't copy binary")?;

    let info_content = format!("name = {}\nversion = {}\n", pkg.name, pkg.version);
    fs::write(format!("{}/info", build_dir), info_content)
        .map_err(|_| "couldn't write info file")?;

    fs::create_dir_all(PACKAGES_DIR)
        .map_err(|_| "couldn't create packages directory (need sudo?)")?;

    let package_file = format!("{}/{}.pls", PACKAGES_DIR, pkg.name);
    create_package(build_dir, &package_file)
        .map_err(|e| format!("couldn't create package: {}", e))?;

    let _ = fs::remove_dir_all(build_dir);

    println!("got ya twin! {} v{} is ready", pkg.name, pkg.version);
    println!("share it: {}", package_file);
    Ok(())
}

fn main() {
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
                cmd_install(&args[2])
            }
        }
        "remove" | "rm" => {
            if args.len() < 3 {
                Err("remove what?".to_string())
            } else {
                cmd_remove(&args[2])
            }
        }
        "info" => {
            if args.len() < 3 {
                Err("info about what?".to_string())
            } else {
                cmd_info(&args[2])
            }
        }
        "list" | "ls" => cmd_list(),
        "add" => {
            let path = if args.len() >= 3 && !args[2].starts_with('-') {
                &args[2]
            } else {
                "."
            };
            let is_draft = args.iter().any(|a| a == "--draft");
            cmd_add(path, is_draft)
        }
        _ => Err(format!("nah '{}' is not a thing, try 'pls help'", command)),
    };

    if let Err(e) = result {
        eprintln!("nah bro: {}", e);
        std::process::exit(1);
    }
}
