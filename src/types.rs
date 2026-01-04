use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io;

#[derive(Serialize, Deserialize)]
pub struct RepoIndex {
    pub version: u32,
    pub updated: String,
    pub packages: HashMap<String, PackageMeta>,
    #[serde(default)]
    pub bundles: HashMap<String, Vec<String>>,
}

#[derive(Serialize, Deserialize)]
pub struct PackageMeta {
    pub version: String,
    pub size: u64,
    pub sha256: String,
    #[serde(default)]
    pub deps: Vec<String>,
    pub desc: String,
}

pub struct PackageInfo {
    pub name: String,
    pub version: String,
    pub depend: Vec<String>,
}

impl PackageInfo {
    pub fn parse_info(content: &str) -> Self {
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
        Self { name, version, depend }
    }

    pub fn from_file(path: &str) -> io::Result<Self> {
        let content = fs::read_to_string(path)?;
        Ok(Self::parse_info(&content))
    }

    pub fn parse_cargo_toml(content: &str) -> Self {
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
        Self { name, version, depend }
    }

    pub fn parse_cmake(content: &str) -> Self {
        let mut name = String::new();
        let mut version = String::new();

        for line in content.lines() {
            let line = line.trim();
            if line.to_lowercase().starts_with("project(") {
                let inner = line
                    .trim_start_matches(|c: char| c != '(')
                    .trim_start_matches('(')
                    .trim_end_matches(')')
                    .trim();

                let parts: Vec<&str> = inner.split_whitespace().collect();
                if !parts.is_empty() {
                    name = parts[0].trim_matches('"').to_string();
                }

                for i in 0..parts.len() {
                    if parts[i].to_uppercase() == "VERSION" && i + 1 < parts.len() {
                        version = parts[i + 1].trim_matches('"').to_string();
                        break;
                    }
                }
            }
        }

        if version.is_empty() {
            version = "0.1.0".to_string();
        }

        Self { name, version, depend: Vec::new() }
    }

    pub fn parse_meson(content: &str) -> Self {
        let mut name = String::new();
        let mut version = String::new();

        for line in content.lines() {
            let line = line.trim();
            if line.starts_with("project(") {
                if let Some(start) = line.find('\'') {
                    if let Some(end) = line[start + 1..].find('\'') {
                        name = line[start + 1..start + 1 + end].to_string();
                    }
                }

                if let Some(ver_pos) = line.find("version:") {
                    let after_ver = &line[ver_pos + 8..];
                    if let Some(start) = after_ver.find('\'') {
                        if let Some(end) = after_ver[start + 1..].find('\'') {
                            version = after_ver[start + 1..start + 1 + end].to_string();
                        }
                    }
                }
            }
        }

        if version.is_empty() {
            version = "0.1.0".to_string();
        }

        Self { name, version, depend: Vec::new() }
    }

    pub fn parse_pls_toml(content: &str) -> Self {
        let mut name = String::new();
        let mut version = String::new();
        let mut depend = Vec::new();

        for line in content.lines() {
            let line = line.trim();
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim().trim_matches('"').trim_matches('\'');
                match key {
                    "name" => name = value.to_string(),
                    "version" => version = value.to_string(),
                    "depend" | "deps" => {
                        if value.starts_with('[') {
                            let inner = value.trim_matches(|c| c == '[' || c == ']');
                            for dep in inner.split(',') {
                                let dep = dep.trim().trim_matches('"').trim_matches('\'');
                                if !dep.is_empty() {
                                    depend.push(dep.to_string());
                                }
                            }
                        } else {
                            depend.push(value.to_string());
                        }
                    }
                    _ => {}
                }
            }
        }

        if version.is_empty() {
            version = "0.1.0".to_string();
        }

        Self { name, version, depend }
    }
}
