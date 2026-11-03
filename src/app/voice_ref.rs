use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(
    tag = "mode",
    content = "value",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
pub enum VoiceVersion {
    #[default]
    Latest,
    Exact(u32),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(deny_unknown_fields)]
pub struct VoiceRef {
    pub identity: String,
    pub language: String,
    pub version: VoiceVersion,
}

impl Default for VoiceRef {
    fn default() -> Self {
        Self::latest(String::new(), "japanese")
    }
}

impl VoiceRef {
    pub fn latest(identity: impl Into<String>, language: impl Into<String>) -> Self {
        Self {
            identity: identity.into(),
            language: language.into(),
            version: VoiceVersion::Latest,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.identity.is_empty()
    }

    pub fn valid(&self) -> bool {
        self.identity.len() <= 4096
            && !self.identity.contains('\0')
            && !self.language.is_empty()
            && self.language.len() <= 128
            && !self.language.contains('\0')
    }
}

impl std::fmt::Display for VoiceRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} [{}; ", self.identity, self.language)?;
        match self.version {
            VoiceVersion::Latest => write!(f, "latest]"),
            VoiceVersion::Exact(version) => write!(f, "v{version}]"),
        }
    }
}

pub(crate) trait VoiceSourceInfo {
    fn identity(&self) -> &str;
    fn languages(&self) -> &[String];
    fn version(&self) -> u32;
}

pub(crate) fn resolve_source<'a, T: VoiceSourceInfo>(
    reference: &VoiceRef,
    sources: &'a [T],
) -> Option<&'a T> {
    let candidates = sources.iter().filter(|source| {
        source.identity() == reference.identity && source.languages().contains(&reference.language)
    });
    match reference.version {
        VoiceVersion::Latest => {
            candidates.reduce(|a, b| if b.version() > a.version() { b } else { a })
        }
        VoiceVersion::Exact(version) => candidates
            .into_iter()
            .find(|source| source.version() == version),
    }
}
