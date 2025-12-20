use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::anyhow;
use toml::Table;
use toml_edit::DocumentMut;

use crate::{Config, Project, SemVer, SemVerBump};

pub struct PyProject {
    version: SemVer,
    path: PathBuf,
}

impl PyProject {
    fn parse_pyproject(pyproject_str: &str) -> anyhow::Result<SemVer> {
        let toml = pyproject_str.parse::<Table>()?;

        let project = toml
            .get("project")
            .and_then(|val| val.as_table())
            .ok_or_else(|| anyhow!("missing [project] section"))?;

        let version_str = project
            .get("version")
            .and_then(|val| val.as_str())
            .ok_or_else(|| anyhow!("missing version in [project] section"))?;

        SemVer::parse(version_str)
    }
}

impl Project for PyProject {
    fn from_dir(path: &Path) -> anyhow::Result<Self> {
        let mut pyproject_path = path.to_path_buf();
        pyproject_path.push("pyproject.toml");
        let raw_file: String = fs::read_to_string(&pyproject_path)?;
        let version = Self::parse_pyproject(&raw_file)?;
        Ok(Self {
            version,
            path: pyproject_path,
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
        doc["project"]["version"] = toml_edit::value(self.version.to_string());
        println!("Updating {:?}", &self.path);
        fs::write(&self.path, doc.to_string())?;
        Ok(())
    }

    fn get_version_file(&self) -> &Path {
        Path::new("pyproject.toml")
    }

    fn set_initial_release(&mut self) -> anyhow::Result<()> {
        if SemVer::version_1_0_0() <= self.get_version() {
            return Err(anyhow!("This repo already has an initial release"));
        }
        self.version = SemVer::version_1_0_0();
        Ok(())
    }

    fn parse_version_file(&self, unparsed_str: &str) -> anyhow::Result<SemVer> {
        Self::parse_pyproject(unparsed_str)
    }
    fn get_extra_files(&self, _config: &Config) -> anyhow::Result<Vec<PathBuf>> {
        Ok(vec![])
    }
}
