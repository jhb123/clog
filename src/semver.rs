use std::{cmp::Ordering, fmt::Display};

use anyhow::anyhow;
use itertools::EitherOrBoth::{Both, Left, Right};
use itertools::Itertools;
use once_cell::sync::Lazy;
use regex::Regex;

static SEMVER_STR: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(?P<major>0|[1-9]\d*)\.(?P<minor>0|[1-9]\d*)\.(?P<patch>0|[1-9]\d*)(?:-(?P<prerelease>(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+(?P<buildmetadata>[0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$").unwrap()
});

#[derive(Debug, PartialEq, Eq, Clone, Copy, PartialOrd, Ord)]
pub enum SemVerBump {
    None,
    Patch,
    Minor,
    Major,
}

#[derive(Debug, Clone, Default, Eq)]
pub struct SemVer {
    major: usize,
    minor: usize,
    patch: usize,
    prerelease: Option<String>,
    build_meta: Option<String>,
}

impl SemVer {
    pub fn new(
        major: usize,
        minor: usize,
        patch: usize,
        prerelease: Option<String>,
        build_meta: Option<String>,
    ) -> Self {
        Self {
            major,
            minor,
            patch,
            prerelease,
            build_meta,
        }
    }

    pub fn version_0_1_0() -> Self {
        Self {
            major: 0,
            minor: 1,
            patch: 0,
            prerelease: None,
            build_meta: None,
        }
    }

    pub fn version_1_0_0() -> Self {
        Self {
            major: 1,
            minor: 0,
            patch: 0,
            prerelease: None,
            build_meta: None,
        }
    }

    pub fn parse(string: &str) -> anyhow::Result<Self> {
        // Recommended Regex from Semver 2.0.0
        // no leading zeros: 0|[1-9]\d*

        let caps = SEMVER_STR
            .captures(string)
            .ok_or_else(|| anyhow!("Project does not follow SemVer"))?;

        let major = caps.name("major").unwrap().as_str().parse()?;
        let minor = caps.name("minor").unwrap().as_str().parse()?;
        let patch = caps.name("patch").unwrap().as_str().parse()?;
        let prerelease = caps.name("prerelease").map(|m| m.as_str().to_string());
        let build_meta = caps.name("buildmetadata").map(|m| m.as_str().to_string());

        Ok(Self {
            major,
            minor,
            patch,
            prerelease,
            build_meta,
        })
    }

    pub fn exact_eq(&self, other: &Self) -> bool {
        self == other && self.build_meta == other.build_meta
    }

    pub fn bump(&self, bump: SemVerBump) -> Self {
        let bump = if self.major == 0 && bump == SemVerBump::Major {
            SemVerBump::Minor
        } else {
            bump
        };

        match bump {
            SemVerBump::Major => SemVer::new(self.major + 1, 0, 0, None, None),
            SemVerBump::Minor => SemVer::new(self.major, self.minor + 1, 0, None, None),
            SemVerBump::Patch => SemVer::new(self.major, self.minor, self.patch + 1, None, None),
            SemVerBump::None => self.clone(),
        }
    }
}

impl Display for SemVer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.prerelease, &self.build_meta) {
            (None, None) => write!(f, "{}.{}.{}", self.major, self.minor, self.patch),
            (None, Some(meta)) => {
                write!(f, "{}.{}.{}+{}", self.major, self.minor, self.patch, meta)
            }
            (Some(pre_release), None) => write!(
                f,
                "{}.{}.{}-{}",
                self.major, self.minor, self.patch, pre_release
            ),
            (Some(pre_release), Some(meta)) => write!(
                f,
                "{}.{}.{}-{}+{}",
                self.major, self.minor, self.patch, pre_release, meta
            ),
        }
    }
}

impl PartialEq for SemVer {
    // according to semver, build meta does not affect precedence,
    // so to keep == following semver, we ignore it.
    fn eq(&self, other: &Self) -> bool {
        self.major == other.major
            && self.minor == other.minor
            && self.patch == other.patch
            && self.prerelease == other.prerelease
    }
}

impl Ord for SemVer {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.major.cmp(&other.major) {
            core::cmp::Ordering::Equal => {}
            ord => return ord,
        }
        match self.minor.cmp(&other.minor) {
            core::cmp::Ordering::Equal => {}
            ord => return ord,
        }
        match self.patch.cmp(&other.patch) {
            core::cmp::Ordering::Equal => {}
            ord => return ord,
        }
        match (&self.prerelease, &other.prerelease) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(a), Some(b)) => compare_prerelease(a, b),
        }
    }
}

impl PartialOrd for SemVer {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other)) // or self.cmp(other).into()
    }
}

fn compare_prerelease(a: &str, b: &str) -> Ordering {
    let a_parts = a.split('.').collect::<Vec<_>>();
    let b_parts = b.split('.').collect::<Vec<_>>();

    for pair in a_parts.iter().zip_longest(b_parts.iter()) {
        match pair {
            Both(a_id, b_id) => {
                // Try to parse both as numbers
                let a_num = a_id.parse::<u64>();
                let b_num = b_id.parse::<u64>();

                match (a_num, b_num) {
                    (Ok(a_n), Ok(b_n)) => match a_n.cmp(&b_n) {
                        Ordering::Equal => continue,
                        non_eq => return non_eq,
                    },
                    (Ok(_), Err(_)) => return Ordering::Less, // numeric < non-numeric
                    (Err(_), Ok(_)) => return Ordering::Greater, // non-numeric > numeric
                    (Err(_), Err(_)) => match a_id.cmp(b_id) {
                        Ordering::Equal => continue,
                        non_eq => return non_eq,
                    },
                }
            }
            Left(_) => return Ordering::Greater, // a has extra identifiers
            Right(_) => return Ordering::Less,   // b has extra identifiers
        }
    }

    Ordering::Equal
}

#[cfg(test)]
mod test {
    use std::cmp::Ordering;

    use crate::semver::{compare_prerelease, SemVer, SemVerBump};

    #[test]
    #[rustfmt::skip]
    fn test_semver_eq() {
        assert_eq!(SemVer::parse("1.1.1").unwrap(),SemVer::parse("1.1.1").unwrap());
        assert_eq!(SemVer::parse("0.1.1").unwrap(),SemVer::parse("0.1.1").unwrap());
        assert_eq!(SemVer::parse("1.0.1").unwrap(),SemVer::parse("1.0.1").unwrap());
        assert_eq!(SemVer::parse("1.1.0").unwrap(),SemVer::parse("1.1.0").unwrap());
        assert_eq!(SemVer::parse("0.0.0").unwrap(),SemVer::parse("0.0.0").unwrap());
        assert_eq!(SemVer::parse("1.1.1+linux").unwrap(),SemVer::parse("1.1.1").unwrap());
        assert_ne!(SemVer::parse("1.1.1-rc").unwrap(),SemVer::parse("1.1.1").unwrap());
    }

    #[test]
    #[rustfmt::skip]
    fn test_precedence() {
        assert!(SemVer::parse("1.0.0-alpha").unwrap() < SemVer::parse("1.0.0-alpha.1").unwrap());
        assert!(SemVer::parse("1.0.0-alpha.1").unwrap() < SemVer::parse("1.0.0-alpha.beta").unwrap());
        assert!(SemVer::parse("1.0.0-alpha.beta").unwrap() < SemVer::parse("1.0.0-beta").unwrap());
        assert!(SemVer::parse("1.0.0-beta").unwrap() < SemVer::parse("1.0.0-beta.2").unwrap());
        assert!(SemVer::parse("1.0.0-beta.2").unwrap() < SemVer::parse("1.0.0-beta.11").unwrap());
        assert!(SemVer::parse("1.0.0-rc.1").unwrap() < SemVer::parse("1.0.0").unwrap());
        assert!(SemVer::parse("1.0.0").unwrap() < SemVer::parse("1.0.1").unwrap());
        assert!(SemVer::parse("1.0.1").unwrap() < SemVer::parse("1.1.0").unwrap());
        assert!(SemVer::parse("1.1.0").unwrap() < SemVer::parse("1.1.1").unwrap());
        assert!(SemVer::parse("1.1.1").unwrap() < SemVer::parse("2.0.0").unwrap());
    }

    #[test]
    fn test_pre_release_precedence() {
        assert_eq!(compare_prerelease("alpha", "beta"), Ordering::Less);
        assert_eq!(compare_prerelease("alpha.0", "alpha.1"), Ordering::Less);
        assert_eq!(compare_prerelease("alpha", "alpha.0"), Ordering::Less);
        assert_eq!(compare_prerelease("alpha", "alpha.1"), Ordering::Less);
        assert_eq!(compare_prerelease("alpha.0", "beta"), Ordering::Less);
        assert_eq!(compare_prerelease("alpha.0", "alpha.beta"), Ordering::Less);
    }

    #[test]
    fn test_build_version_does_not_affect_precedance() {
        let v1 = SemVer::parse("1.0.0+build.1").unwrap();
        let v2 = SemVer::parse("1.0.0+build.2").unwrap();
        assert_eq!(v1, v2);
    }

    #[test]
    fn test_major_bump() {
        // initial version 0 case
        assert_eq!(
            SemVer::parse("0.0.0").unwrap().bump(SemVerBump::Major),
            SemVer::parse("0.1.0").unwrap()
        );

        assert_eq!(
            SemVer::parse("1.0.1").unwrap().bump(SemVerBump::Major),
            SemVer::parse("2.0.0").unwrap()
        );
        assert_eq!(
            SemVer::parse("1.1.0").unwrap().bump(SemVerBump::Major),
            SemVer::parse("2.0.0").unwrap()
        );
        assert_eq!(
            SemVer::parse("1.1.0-rc1").unwrap().bump(SemVerBump::Major),
            SemVer::parse("2.0.0").unwrap()
        );
        assert_eq!(
            SemVer::parse("0.0.0").unwrap().bump(SemVerBump::Minor),
            SemVer::parse("0.1.0").unwrap()
        );
        assert_eq!(
            SemVer::parse("0.0.1").unwrap().bump(SemVerBump::Minor),
            SemVer::parse("0.1.0").unwrap()
        );
        assert_eq!(
            SemVer::parse("0.1.0").unwrap().bump(SemVerBump::Minor),
            SemVer::parse("0.2.0").unwrap()
        );
        assert_eq!(
            SemVer::parse("1.1.0-rc1").unwrap().bump(SemVerBump::Minor),
            SemVer::parse("1.2.0").unwrap()
        );
        assert_eq!(
            SemVer::parse("1.0.1").unwrap().bump(SemVerBump::Patch),
            SemVer::parse("1.0.2").unwrap()
        );
        assert_eq!(
            SemVer::parse("1.1.0").unwrap().bump(SemVerBump::Patch),
            SemVer::parse("1.1.1").unwrap()
        );
        assert_eq!(
            SemVer::parse("1.1.0-rc1").unwrap().bump(SemVerBump::Patch),
            SemVer::parse("1.1.1").unwrap()
        );
        assert_eq!(
            SemVer::parse("1.1.0-rc1").unwrap().bump(SemVerBump::None),
            SemVer::parse("1.1.0-rc1").unwrap()
        );

        assert_eq!(
            SemVer::parse("0.1.0").unwrap().bump(SemVerBump::None),
            SemVer::parse("0.1.0").unwrap()
        );

        assert_eq!(
            SemVer::parse("0.0.1").unwrap().bump(SemVerBump::None),
            SemVer::parse("0.0.1").unwrap()
        );

        assert_eq!(
            SemVer::parse("1.0.0").unwrap().bump(SemVerBump::None),
            SemVer::parse("1.0.0").unwrap()
        );
    }

    #[test]
    fn test_bump_order() {
        assert!(SemVerBump::Major > SemVerBump::Minor);
        assert!(SemVerBump::Minor > SemVerBump::Patch);
        assert!(SemVerBump::Patch > SemVerBump::None);
    }
}
