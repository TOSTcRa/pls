use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::network::{fetch_index, resolve_or_download};
use crate::types::{PackageInfo, PackageMeta, RepoIndex};
use crate::utils::{calculate_sha256, create_package, extract_package, is_installed};
use crate::{DB_DIR, PACKAGES_DIR, ROOT};

pub async fn cmd_install(package_input: &str) -> Result<(), String> {
    let package_path = resolve_or_download(package_input).await?;

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

    let bin_dir = format!("{}/bin", temp_dir);
    let entries = fs::read_dir(&bin_dir)
        .map_err(|e| format!("couldn't read bin dir: {}", e))?;

    for entry in entries.flatten() {
        let src = entry.path();
        if src.is_file() {
            let filename = entry.file_name();
            let dest = format!("{}/usr/bin/{}", ROOT, filename.to_string_lossy());
            let _ = fs::remove_file(&dest);
            fs::copy(&src, &dest)
                .map_err(|e| format!("couldn't copy {}: {}", filename.to_string_lossy(), e))?;
        }
    }

    let db_path = format!("{}/{}", DB_DIR, pkg.name);
    fs::create_dir_all(&db_path).map_err(|e| format!("couldn't create db entry: {}", e))?;
    fs::copy(format!("{}/info", temp_dir), format!("{}/info", db_path))
        .map_err(|e| format!("couldn't save package info: {}", e))?;

    let _ = fs::remove_dir_all(temp_dir);

    println!("got ya! {} v{} installed", pkg.name, pkg.version);
    Ok(())
}

pub fn cmd_remove(package_name: &str) -> Result<(), String> {
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

pub fn cmd_info(package_input: &str) -> Result<(), String> {
    let package_path = crate::utils::resolve_package_path(package_input)
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

pub fn cmd_list() -> Result<(), String> {
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

pub enum ProjectType {
    Rust,
    CMake,
    Meson,
    PlsToml,
}

fn detect_project(path: &str) -> Option<(ProjectType, PackageInfo)> {
    let cargo_path = format!("{}/Cargo.toml", path);
    if Path::new(&cargo_path).exists() {
        if let Ok(content) = fs::read_to_string(&cargo_path) {
            let pkg = PackageInfo::parse_cargo_toml(&content);
            if !pkg.name.is_empty() {
                return Some((ProjectType::Rust, pkg));
            }
        }
    }

    let cmake_path = format!("{}/CMakeLists.txt", path);
    if Path::new(&cmake_path).exists() {
        if let Ok(content) = fs::read_to_string(&cmake_path) {
            let pkg = PackageInfo::parse_cmake(&content);
            if !pkg.name.is_empty() {
                return Some((ProjectType::CMake, pkg));
            }
        }
    }

    let meson_path = format!("{}/meson.build", path);
    if Path::new(&meson_path).exists() {
        if let Ok(content) = fs::read_to_string(&meson_path) {
            let pkg = PackageInfo::parse_meson(&content);
            if !pkg.name.is_empty() {
                return Some((ProjectType::Meson, pkg));
            }
        }
    }

    let pls_path = format!("{}/pls.toml", path);
    if Path::new(&pls_path).exists() {
        if let Ok(content) = fs::read_to_string(&pls_path) {
            let pkg = PackageInfo::parse_pls_toml(&content);
            if !pkg.name.is_empty() {
                return Some((ProjectType::PlsToml, pkg));
            }
        }
    }

    None
}

pub fn cmd_add(project_path: &str, is_draft: bool, output_dir: Option<&str>) -> Result<(), String> {
    let (project_type, pkg) = detect_project(project_path)
        .ok_or_else(|| "dunno what project this is, need Cargo.toml, CMakeLists.txt, meson.build, or pls.toml".to_string())?;

    let binary_path = match project_type {
        ProjectType::Rust => {
            let (build_type, bin_path) = if is_draft {
                ("debug", format!("{}/target/debug/{}", project_path, pkg.name))
            } else {
                ("release", format!("{}/target/release/{}", project_path, pkg.name))
            };

            if !Path::new(&bin_path).exists() {
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
            bin_path
        }
        ProjectType::CMake => {
            let build_dir = format!("{}/build", project_path);
            let bin_path = format!("{}/{}", build_dir, pkg.name);

            if !Path::new(&bin_path).exists() {
                println!("building {} with cmake...", pkg.name);
                fs::create_dir_all(&build_dir).map_err(|_| "couldn't create build dir")?;

                let cmake_type = if is_draft { "Debug" } else { "Release" };
                let status = Command::new("cmake")
                    .args(["..", &format!("-DCMAKE_BUILD_TYPE={}", cmake_type)])
                    .current_dir(&build_dir)
                    .status()
                    .map_err(|e| format!("cmake failed: {}", e))?;
                if !status.success() {
                    return Err("cmake configure failed".to_string());
                }

                let status = Command::new("make")
                    .args(["-j4"])
                    .current_dir(&build_dir)
                    .status()
                    .map_err(|e| format!("make failed: {}", e))?;
                if !status.success() {
                    return Err("build failed".to_string());
                }
            }
            bin_path
        }
        ProjectType::Meson => {
            let build_dir = format!("{}/builddir", project_path);
            let bin_path = format!("{}/{}", build_dir, pkg.name);

            if !Path::new(&bin_path).exists() {
                println!("building {} with meson...", pkg.name);

                let build_type = if is_draft { "debug" } else { "release" };
                let status = Command::new("meson")
                    .args(["setup", &build_dir, "--buildtype", build_type])
                    .current_dir(project_path)
                    .status()
                    .map_err(|e| format!("meson failed: {}", e))?;
                if !status.success() {
                    return Err("meson setup failed".to_string());
                }

                let status = Command::new("ninja")
                    .args(["-C", &build_dir])
                    .status()
                    .map_err(|e| format!("ninja failed: {}", e))?;
                if !status.success() {
                    return Err("build failed".to_string());
                }
            }
            bin_path
        }
        ProjectType::PlsToml => {
            let pls_path = format!("{}/pls.toml", project_path);
            let content = fs::read_to_string(&pls_path).map_err(|_| "couldn't read pls.toml")?;
            let mut binary = String::new();
            for line in content.lines() {
                if let Some((key, value)) = line.split_once('=') {
                    if key.trim() == "binary" {
                        binary = value.trim().trim_matches('"').trim_matches('\'').to_string();
                    }
                }
            }
            if binary.is_empty() {
                return Err("pls.toml needs 'binary' field".to_string());
            }
            format!("{}/{}", project_path, binary)
        }
    };

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

    let output_path = output_dir.unwrap_or(PACKAGES_DIR);
    fs::create_dir_all(output_path)
        .map_err(|_| "couldn't create output directory (need sudo?)")?;

    let package_file = format!("{}/{}.pls", output_path, pkg.name);
    create_package(build_dir, &package_file)
        .map_err(|e| format!("couldn't create package: {}", e))?;

    let _ = fs::remove_dir_all(build_dir);

    println!("got ya twin! {} v{} is ready", pkg.name, pkg.version);
    println!("share it: {}", package_file);
    Ok(())
}

pub fn cmd_repo_update() -> Result<(), String> {
    let current_dir = env::current_dir().map_err(|_| "couldn't get current directory")?;

    let packages_dir = current_dir.join("packages");
    let index_path = current_dir.join("index.json");

    if !packages_dir.exists() {
        return Err("no packages/ folder here, are you in a repo?".to_string());
    }

    println!("scanning packages/...");

    let mut packages: HashMap<String, PackageMeta> = HashMap::new();
    let temp_dir = "/tmp/pls-repo-scan";

    let entries = fs::read_dir(&packages_dir)
        .map_err(|e| format!("couldn't read packages/: {}", e))?;

    for entry in entries.flatten() {
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) != Some("pls") {
            continue;
        }

        let path_str = path.to_string_lossy().to_string();

        let sha256 = calculate_sha256(&path_str)
            .map_err(|e| format!("couldn't hash {}: {}", path_str, e))?;

        let size = fs::metadata(&path)
            .map_err(|e| format!("couldn't get size of {}: {}", path_str, e))?
            .len();

        extract_package(&path_str, temp_dir)
            .map_err(|e| format!("couldn't extract {}: {}", path_str, e))?;

        let pkg = PackageInfo::from_file(&format!("{}/info", temp_dir))
            .map_err(|e| format!("couldn't read info from {}: {}", path_str, e))?;

        println!("  found {} v{} ({} bytes)", pkg.name, pkg.version, size);

        packages.insert(pkg.name.clone(), PackageMeta {
            version: pkg.version,
            size,
            sha256,
            deps: pkg.depend,
            desc: format!("{} package", pkg.name),
        });
    }

    let _ = fs::remove_dir_all(temp_dir);

    if packages.is_empty() {
        println!("no packages found in packages/");
        return Ok(());
    }

    let existing_bundles: HashMap<String, Vec<String>> = fs::read_to_string(&index_path)
        .ok()
        .and_then(|content| serde_json::from_str::<RepoIndex>(&content).ok())
        .map(|idx| idx.bundles)
        .unwrap_or_default();

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    let index = RepoIndex {
        version: 1,
        updated: today,
        packages,
        bundles: existing_bundles,
    };

    let json = serde_json::to_string_pretty(&index)
        .map_err(|e| format!("couldn't serialize index: {}", e))?;

    fs::write(&index_path, json)
        .map_err(|e| format!("couldn't write index.json: {}", e))?;

    println!("done! index.json updated with {} package(s)", index.packages.len());
    Ok(())
}

pub async fn cmd_update() -> Result<(), String> {
    if !Path::new(DB_DIR).exists() {
        println!("nothing installed yet, nothing to update");
        return Ok(());
    }

    println!("checking for updates...");

    let index = fetch_index().await?;

    let entries = fs::read_dir(DB_DIR).map_err(|_| "couldn't read package database")?;

    let mut installed: Vec<(String, String)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let info_path = format!("{}/{}/info", DB_DIR, name);
        if let Ok(pkg) = PackageInfo::from_file(&info_path) {
            installed.push((pkg.name, pkg.version));
        }
    }

    if installed.is_empty() {
        println!("nothing installed yet");
        return Ok(());
    }

    let mut to_update: Vec<String> = Vec::new();

    for (name, local_version) in &installed {
        if let Some(remote) = index.packages.get(name) {
            if remote.version != *local_version {
                println!("  {} {} -> {}", name, local_version, remote.version);
                to_update.push(name.clone());
            }
        }
    }

    if to_update.is_empty() {
        println!("everything up to date!");
        return Ok(());
    }

    println!("\nupdating {} package(s)...\n", to_update.len());

    let mut updated = 0;
    let mut failed: Vec<String> = Vec::new();

    for pkg in &to_update {
        println!(">>> updating {}...", pkg);
        match cmd_install(pkg).await {
            Ok(_) => updated += 1,
            Err(e) => {
                println!("!!! failed to update {}: {}", pkg, e);
                failed.push(pkg.clone());
            }
        }
        println!();
    }

    if failed.is_empty() {
        println!("nice! {} package(s) updated", updated);
    } else {
        println!("{} updated, {} failed", updated, failed.len());
        println!("failed: {}", failed.join(", "));
    }

    Ok(())
}

pub async fn cmd_bundle(bundle_name: &str) -> Result<(), String> {
    println!("checking repo for bundle '{}'...", bundle_name);

    let index = fetch_index().await?;

    let packages = index
        .bundles
        .get(bundle_name)
        .ok_or_else(|| format!("bundle '{}' not found in repo", bundle_name))?;

    if packages.is_empty() {
        return Err(format!("bundle '{}' is empty", bundle_name));
    }

    println!("installing {} package(s) from bundle '{}':", packages.len(), bundle_name);
    for pkg in packages {
        println!("  - {}", pkg);
    }
    println!();

    let mut failed: Vec<String> = Vec::new();
    let mut installed = 0;

    for pkg in packages {
        println!(">>> installing {}...", pkg);
        match cmd_install(pkg).await {
            Ok(_) => installed += 1,
            Err(e) => {
                println!("!!! failed to install {}: {}", pkg, e);
                failed.push(pkg.clone());
            }
        }
        println!();
    }

    if failed.is_empty() {
        println!("nice! bundle '{}' installed ({} packages)", bundle_name, installed);
    } else {
        println!(
            "bundle '{}' partially installed: {} ok, {} failed",
            bundle_name,
            installed,
            failed.len()
        );
        println!("failed packages: {}", failed.join(", "));
    }

    Ok(())
}
