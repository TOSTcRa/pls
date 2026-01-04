use sha2::{Digest, Sha256};
use std::fs;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use tar::Archive;

use crate::DB_DIR;

pub fn extract_package(archive_path: &str, dest: &str) -> io::Result<()> {
    let _ = fs::remove_dir_all(dest);
    fs::create_dir_all(dest)?;

    let file = File::open(archive_path)?;
    let decoder = zstd::stream::Decoder::new(file)?;
    let mut archive = Archive::new(decoder);
    archive.unpack(dest)?;
    Ok(())
}

pub fn create_package(source_dir: &str, output_path: &str) -> io::Result<()> {
    let file = File::create(output_path)?;
    let encoder = zstd::stream::Encoder::new(file, 3)?;
    let mut tar = tar::Builder::new(encoder);
    tar.append_dir_all(".", source_dir)?;
    let encoder = tar.into_inner()?;
    encoder.finish()?;
    Ok(())
}

pub fn resolve_package_path(input: &str) -> Option<String> {
    if input.contains('/') || input.ends_with(".pls") {
        if Path::new(input).exists() {
            return Some(input.to_string());
        }
    }
    None
}

pub fn is_installed(name: &str) -> bool {
    Path::new(&format!("{}/{}", DB_DIR, name)).exists()
}

pub fn calculate_sha256(path: &str) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hex::encode(hasher.finalize()))
}
