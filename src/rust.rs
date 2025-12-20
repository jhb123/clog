use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
};

use anyhow::{anyhow, Ok};
use toml::Table;
use toml_edit::DocumentMut;

use crate::{Config, Project, SemVer, SemVerBump};

pub struct CargoProject {
    version: SemVer,
    path: PathBuf,
}

impl CargoProject {
    fn parse_cargo(cargo_str: &str) -> anyhow::Result<SemVer> {
        let toml = cargo_str.parse::<Table>()?;

        let project = toml
            .get("package")
            .and_then(|val| val.as_table())
            .ok_or_else(|| anyhow!("missing [project] section"))?;

        let version_str = project
            .get("version")
            .and_then(|val| val.as_str())
            .ok_or_else(|| anyhow!("missing version in [project] section"))?;

        SemVer::parse(version_str)
    }
}

impl Project for CargoProject {
    fn from_dir(path: &Path) -> anyhow::Result<Self> {
        let mut cargo_path = path.to_path_buf();
        cargo_path.push("cargo.toml");
        let raw_file: String = fs::read_to_string(&cargo_path)?;

        let version = Self::parse_cargo(&raw_file)?;

        Ok(Self {
            version,
            path: cargo_path,
        })
    }

    fn get_dir(&self) -> &Path {
        self.path.parent().expect("Project must be in a directory")
    }

    fn get_version(&self) -> SemVer {
        self.version.clone()
    }

    fn bump(&mut self, bump: SemVerBump) {
        self.version = self.version.bump(bump);
    }

    fn write(&self) -> anyhow::Result<()> {
        let content = fs::read_to_string(&self.path)?;
        let mut doc = content.parse::<DocumentMut>()?;
        doc["package"]["version"] = toml_edit::value(self.version.to_string());
        fs::write(&self.path, doc.to_string())?;
        Ok(())
    }

    fn get_version_file(&self) -> &Path {
        Path::new("Cargo.toml")
    }

    fn set_initial_release(&mut self) -> anyhow::Result<()> {
        if SemVer::version_1_0_0() <= self.get_version() {
            return Err(anyhow!("This repo already has an initial release"));
        }
        self.version = SemVer::version_1_0_0();
        Ok(())
    }

    fn parse_version_file(&self, unparsed_str: &str) -> anyhow::Result<SemVer> {
        Self::parse_cargo(unparsed_str)
    }

    fn get_extra_files(&self, _config: &Config) -> anyhow::Result<Vec<PathBuf>> {
        let status = Command::new("cargo")
            .arg("generate-lockfile")
            .status()
            .expect("failed to run cargo generate-lockfile");
        if !status.success() {
            return Err(anyhow::anyhow!("Failed to generate lockfile"));
        }
        Ok(vec![PathBuf::from_str("Cargo.lock")?])
    }
}
