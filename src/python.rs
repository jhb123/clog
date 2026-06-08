use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::anyhow;
use toml::Table;
use toml_edit::DocumentMut;

use crate::{Config, Project, SemVer};

enum PyProjectFormat {
    Pep,
    Poetry,
}

pub struct PyProject {
    version: SemVer,
    path: PathBuf,
    format: PyProjectFormat,
}

impl PyProject {
    fn parse_pyproject(pyproject_str: &str) -> anyhow::Result<(SemVer, PyProjectFormat)> {
        let toml = pyproject_str.parse::<Table>()?;

        if let Some(version_str) = toml
            .get("project")
            .and_then(|v| v.as_table())
            .and_then(|t| t.get("version"))
            .and_then(|v| v.as_str())
        {
            return Ok((SemVer::parse(version_str)?, PyProjectFormat::Pep));
        }

        if let Some(version_str) = toml
            .get("tool")
            .and_then(|v| v.as_table())
            .and_then(|t| t.get("poetry"))
            .and_then(|v| v.as_table())
            .and_then(|t| t.get("version"))
            .and_then(|v| v.as_str())
        {
            return Ok((SemVer::parse(version_str)?, PyProjectFormat::Poetry));
        }

        Err(anyhow!(
            "missing version: expected [project] or [tool.poetry] section"
        ))
    }
}

impl Project for PyProject {
    fn from_dir(path: &Path) -> anyhow::Result<Self> {
        let mut pyproject_path = path.to_path_buf();
        pyproject_path.push("pyproject.toml");
        let raw_file: String = fs::read_to_string(&pyproject_path)?;
        let (version, format) = Self::parse_pyproject(&raw_file)?;
        Ok(Self {
            version,
            path: pyproject_path,
            format,
        })
    }

    fn get_dir(&self) -> &Path {
        self.path.parent().expect("Project must be in a directory")
    }

    fn get_version(&self) -> SemVer {
        self.version.clone()
    }

    fn set_version(&mut self, version: SemVer) {
        self.version = version;
    }

    fn update_project_file(&self) -> anyhow::Result<()> {
        let content = fs::read_to_string(&self.path)?;
        let mut doc = content.parse::<DocumentMut>()?;
        match self.format {
            PyProjectFormat::Pep => {
                doc["project"]["version"] = toml_edit::value(self.version.to_string());
            }
            PyProjectFormat::Poetry => {
                doc["tool"]["poetry"]["version"] = toml_edit::value(self.version.to_string());
            }
        }
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
        let (version, _) = Self::parse_pyproject(unparsed_str)?;
        Ok(version)
    }

    fn get_extra_files(&self, _config: &Config) -> anyhow::Result<Vec<PathBuf>> {
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEP: &str = r#"
[build-system]
requires = ["setuptools"]
build-backend = "setuptools.build_meta"

[project]
name = "example"
version = "1.2.3"
"#;

    const POETRY: &str = r#"
[tool.poetry]
name = "example"
version = "1.2.3"
description = ""

[build-system]
requires = ["poetry-core"]
build-backend = "poetry.core.masonry.api"
"#;

    #[test]
    fn parse_pep() {
        let (v, _) = PyProject::parse_pyproject(PEP).unwrap();
        assert_eq!(v, SemVer::parse("1.2.3").unwrap());
    }

    #[test]
    fn parse_poetry() {
        let (v, _) = PyProject::parse_pyproject(POETRY).unwrap();
        assert_eq!(v, SemVer::parse("1.2.3").unwrap());
    }

    #[test]
    fn parse_missing_version_fails() {
        assert!(PyProject::parse_pyproject("[build-system]\nrequires = []").is_err());
    }

    #[test]
    fn update_pep() {
        let (_, format) = PyProject::parse_pyproject(PEP).unwrap();
        let mut doc = PEP.parse::<DocumentMut>().unwrap();
        match format {
            PyProjectFormat::Pep => doc["project"]["version"] = toml_edit::value("9.9.9"),
            PyProjectFormat::Poetry => doc["tool"]["poetry"]["version"] = toml_edit::value("9.9.9"),
        }
        let updated = doc.to_string();
        let (v, _) = PyProject::parse_pyproject(&updated).unwrap();
        assert_eq!(v, SemVer::parse("9.9.9").unwrap());
        assert!(updated.contains(r#"version = "9.9.9""#));
    }

    #[test]
    fn update_poetry() {
        let (_, format) = PyProject::parse_pyproject(POETRY).unwrap();
        let mut doc = POETRY.parse::<DocumentMut>().unwrap();
        match format {
            PyProjectFormat::Pep => doc["project"]["version"] = toml_edit::value("9.9.9"),
            PyProjectFormat::Poetry => doc["tool"]["poetry"]["version"] = toml_edit::value("9.9.9"),
        }
        let updated = doc.to_string();
        let (v, _) = PyProject::parse_pyproject(&updated).unwrap();
        assert_eq!(v, SemVer::parse("9.9.9").unwrap());
        assert!(updated.contains(r#"version = "9.9.9""#));
    }
}
