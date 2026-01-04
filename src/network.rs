use std::fs;
use std::path::Path;
use std::process::Command;

use crate::types::RepoIndex;
use crate::utils::{create_package, resolve_package_path};
use crate::{CACHE_DIR, REPO_URL};

pub async fn fetch_index() -> Result<RepoIndex, String> {
    let res = reqwest::get(format!("{}/index.json", REPO_URL))
        .await
        .map_err(|e| e.to_string())?;
    let text = res.text().await.map_err(|e| e.to_string())?;
    let index = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    Ok(index)
}

pub async fn download_package(name: &str) -> Result<String, String> {
    let url = format!("{}/packages/{}.pls", REPO_URL, name.trim());
    let res = reqwest::get(url).await.map_err(|e| e.to_string())?;
    let bytes = res.bytes().await.map_err(|e| e.to_string())?;
    fs::create_dir_all(CACHE_DIR).map_err(|e| e.to_string())?;
    let file_path = format!("{}/{}.pls", CACHE_DIR, name);
    fs::write(&file_path, bytes).map_err(|e| e.to_string())?;
    Ok(file_path)
}

pub async fn download_deb(url: &str, name: &str) -> Result<String, String> {
    println!("downloading from debian...");
    let res = reqwest::get(url).await.map_err(|e| e.to_string())?;

    if !res.status().is_success() {
        return Err(format!("failed to download: {}", res.status()));
    }

    let bytes = res.bytes().await.map_err(|e| e.to_string())?;

    let deb_dir = "/tmp/pls-deb";
    let _ = fs::remove_dir_all(deb_dir);
    fs::create_dir_all(deb_dir).map_err(|e| e.to_string())?;

    let deb_path = format!("{}/package.deb", deb_dir);
    fs::write(&deb_path, &bytes).map_err(|e| e.to_string())?;

    let status = Command::new("ar")
        .args(["x", &deb_path])
        .current_dir(deb_dir)
        .status()
        .map_err(|_| "ar not found, install binutils")?;

    if !status.success() {
        return Err("failed to extract .deb".to_string());
    }

    let data_tar = if Path::new(&format!("{}/data.tar.xz", deb_dir)).exists() {
        format!("{}/data.tar.xz", deb_dir)
    } else if Path::new(&format!("{}/data.tar.zst", deb_dir)).exists() {
        format!("{}/data.tar.zst", deb_dir)
    } else if Path::new(&format!("{}/data.tar.gz", deb_dir)).exists() {
        format!("{}/data.tar.gz", deb_dir)
    } else {
        return Err("couldn't find data.tar in .deb".to_string());
    };

    let extract_dir = format!("{}/extract", deb_dir);
    fs::create_dir_all(&extract_dir).map_err(|e| e.to_string())?;

    let status = Command::new("tar")
        .args(["xf", &data_tar, "-C", &extract_dir])
        .status()
        .map_err(|e| e.to_string())?;

    if !status.success() {
        return Err("failed to extract data.tar".to_string());
    }

    let build_dir = "/tmp/pls-deb-build";
    let _ = fs::remove_dir_all(build_dir);
    fs::create_dir_all(format!("{}/bin", build_dir)).map_err(|e| e.to_string())?;

    let bin_dirs = [
        format!("{}/usr/bin", extract_dir),
        format!("{}/usr/local/bin", extract_dir),
        format!("{}/bin", extract_dir),
    ];

    let mut found_binary = false;
    for bin_dir in &bin_dirs {
        if Path::new(bin_dir).exists() {
            if let Ok(entries) = fs::read_dir(bin_dir) {
                for entry in entries.flatten() {
                    let src = entry.path();
                    if src.is_file() {
                        let dest = format!("{}/bin/{}", build_dir, entry.file_name().to_string_lossy());
                        let _ = fs::copy(&src, &dest);
                        found_binary = true;
                    }
                }
            }
        }
    }

    if !found_binary {
        return Err("no binaries found in .deb".to_string());
    }

    let info_content = format!("name = {}\nversion = 1.0.0\n", name);
    fs::write(format!("{}/info", build_dir), info_content).map_err(|e| e.to_string())?;

    fs::create_dir_all(CACHE_DIR).map_err(|e| e.to_string())?;
    let pls_path = format!("{}/{}.pls", CACHE_DIR, name);
    create_package(build_dir, &pls_path).map_err(|e| e.to_string())?;

    let _ = fs::remove_dir_all(deb_dir);
    let _ = fs::remove_dir_all(build_dir);

    println!("converted deb to pls!");
    Ok(pls_path)
}

pub async fn resolve_or_download(name: &str) -> Result<String, String> {
    if let Some(path) = resolve_package_path(name) {
        return Ok(path);
    }

    if name.ends_with(".deb") || name.starts_with("http") {
        let url = name;
        let pkg_name = name
            .split('/')
            .last()
            .unwrap_or(name)
            .trim_end_matches(".deb")
            .split('_')
            .next()
            .unwrap_or(name);
        return download_deb(url, pkg_name).await;
    }

    println!("lemme check the repo...");
    let index = fetch_index().await?;

    if index.packages.contains_key(name) {
        println!("downloading {}...", name);
        return download_package(name).await;
    }

    Err(format!("'{}' not found in repo. try: pls install <url-to-deb>", name))
}
