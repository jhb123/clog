use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Ok};
use toml::Table;
use toml_edit::DocumentMut;

use crate::{Project, SemVer, SemVerBump};

pub struct CargoProject {
    version: SemVer,
    path: PathBuf,
    root: PathBuf,
}

impl Project for CargoProject {
    fn from_dir(path: &Path) -> anyhow::Result<Self> {
        let mut cargo_path = path.to_path_buf();
        cargo_path.push("cargo.toml");
        let raw_file: String = fs::read_to_string(path)?;

        let toml = raw_file.parse::<Table>()?;

        let project = toml
            .get("project")
            .and_then(|val| val.as_table())
            .ok_or_else(|| anyhow!("missing [project] section"))?;

        let version_str = project
            .get("version")
            .and_then(|val| val.as_str())
            .ok_or_else(|| anyhow!("missing version in [project] section"))?;

        let version = SemVer::parse(version_str)?;

        Ok(Self {
            version,
            path: cargo_path,
            root: path.to_path_buf(),
        })
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
        doc["project"]["version"] = toml_edit::value(self.version.to_string());
        fs::write(&self.path, doc.to_string())?;
        Ok(())
    }

    fn get_version_file(&self) -> PathBuf {
        self.path.strip_prefix(&self.root).unwrap().to_path_buf()
    }

    fn set_initial_release(&mut self) -> anyhow::Result<()> {
        if SemVer::version_1_0_0() <= self.get_version() {
            return Err(anyhow!("This repo already has an initial release"));
        }
        self.version = SemVer::version_1_0_0();
        Ok(())
    }
}
