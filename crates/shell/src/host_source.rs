//! An immutable in-memory JavaScript module graph for hosted evaluation.
//!
//! [`HostSource`] is a sealed specifier-to-bytes map. After construction it has
//! no mutating API. [`crate::ShellRuntime::load_host_source`] evaluates it
//! without writing editor files or materializing Git dependencies.

use std::{collections::BTreeMap, path::Path, sync::Arc};

use anyhow::{Result, bail};

/// Maximum size of one JavaScript module, in bytes.
pub const MAX_MODULE_BYTES: u64 = 8 * 1024 * 1024;

/// Frozen in-memory application source: an entry specifier and its module map.
#[derive(Clone, Debug)]
pub struct HostSource {
    inner: Arc<HostSourceInner>,
}

#[derive(Debug)]
struct HostSourceInner {
    entry: String,
    modules: BTreeMap<String, String>,
}

impl HostSource {
    /// Seals `modules` under `entry`.
    ///
    /// Specifiers are normalized (`./util.js` becomes `util.js`). Construction
    /// rejects an empty entry, duplicates after normalization, `..` components,
    /// absolute paths, NUL bytes, modules larger than [`MAX_MODULE_BYTES`], and
    /// an entry that is not present in the map.
    ///
    /// # Errors
    ///
    /// Returns a description of the first illegal specifier or module body.
    pub fn new(
        entry: impl AsRef<str>,
        modules: impl IntoIterator<Item = (impl AsRef<str>, impl AsRef<[u8]>)>,
    ) -> Result<Self> {
        let entry = normalize_module_specifier(entry.as_ref())?;
        let mut sealed = BTreeMap::new();
        for (specifier, bytes) in modules {
            let specifier = normalize_module_specifier(specifier.as_ref())?;
            let bytes = bytes.as_ref();
            if bytes.contains(&0) {
                bail!("module `{specifier}` contains a NUL byte");
            }
            let size = bytes.len() as u64;
            if size > MAX_MODULE_BYTES {
                bail!(
                    "module `{specifier}` is {size} bytes, over the {MAX_MODULE_BYTES}-byte limit"
                );
            }
            let source = std::str::from_utf8(bytes)
                .map_err(|_| anyhow::anyhow!("module `{specifier}` is not valid UTF-8"))?;
            if sealed
                .insert(specifier.clone(), source.to_owned())
                .is_some()
            {
                bail!("duplicate module specifier `{specifier}`");
            }
        }
        if !sealed.contains_key(&entry) {
            bail!("entry `{entry}` is not present in the module map");
        }
        Ok(Self {
            inner: Arc::new(HostSourceInner {
                entry,
                modules: sealed,
            }),
        })
    }

    /// The normalized entry specifier.
    #[must_use]
    pub fn entry(&self) -> &str {
        &self.inner.entry
    }

    /// Bytes for a normalized specifier, if the map contains it.
    #[must_use]
    pub fn get(&self, specifier: &str) -> Option<&[u8]> {
        self.source(specifier).map(str::as_bytes)
    }

    pub(crate) fn source(&self, specifier: &str) -> Option<&str> {
        self.inner.modules.get(specifier).map(String::as_str)
    }

    pub(crate) fn contains(&self, specifier: &str) -> bool {
        self.inner.modules.contains_key(specifier)
    }
}

pub(crate) fn normalize_module_specifier(raw: &str) -> Result<String> {
    if raw.is_empty() {
        bail!("module specifier cannot be empty");
    }
    if raw.contains('\0') {
        bail!("module specifier `{raw}` contains a NUL byte");
    }
    if is_absolute_specifier(raw) {
        bail!("module specifier `{raw}` is an absolute path");
    }
    let normalized = raw.replace('\\', "/");
    let mut parts = Vec::new();
    for component in normalized.split('/') {
        if component == ".." {
            bail!("module specifier `{raw}` contains `..`");
        }
        if component.is_empty() || component == "." {
            continue;
        }
        parts.push(component);
    }
    if parts.is_empty() {
        bail!("module specifier `{raw}` is empty after normalization");
    }
    Ok(parts.join("/"))
}

fn is_absolute_specifier(raw: &str) -> bool {
    if Path::new(raw).is_absolute() {
        return true;
    }
    let bytes = raw.as_bytes();
    if bytes
        .first()
        .is_some_and(|byte| *byte == b'/' || *byte == b'\\')
    {
        return true;
    }
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

#[cfg(test)]
mod tests {
    use super::HostSource;

    #[test]
    fn rejects_empty_entry_parent_absolute_nul_and_duplicates() {
        assert!(HostSource::new("", [("main.js", b"x".as_slice())]).is_err());
        assert!(
            HostSource::new(
                "main.js",
                [("main.js", b"x".as_slice()), ("../x.js", b"y".as_slice()),]
            )
            .is_err()
        );
        assert!(HostSource::new("/abs.js", [("/abs.js", b"x".as_slice())]).is_err());
        assert!(HostSource::new("main.js\0", [("main.js\0", b"x".as_slice())]).is_err());
        assert!(
            HostSource::new(
                "main.js",
                [("main.js", b"a".as_slice()), ("./main.js", b"b".as_slice()),]
            )
            .is_err()
        );
    }
}
