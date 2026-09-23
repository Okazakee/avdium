//! A minimally changing INI representation.
//!
//! Android AVD configuration files are plain INI. Users and other tools edit
//! them, so Avdium must not reformat, reorder, or drop anything it does not
//! understand. This parser keeps every line, including comments, blank lines,
//! unknown keys, duplicate keys, and unparsable lines, and records problems
//! instead of silently normalizing them away.

use std::fmt;

/// Line ending style of the source document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Newline {
    /// `\n`.
    #[default]
    Lf,
    /// `\r\n`.
    CrLf,
}

impl Newline {
    /// The characters to write.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
        }
    }
}

/// One line of an INI document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IniEntry {
    /// A `key=value` line. Both parts are stored exactly as written.
    Pair {
        /// The key, before the first `=`.
        key: String,
        /// Everything after the first `=`.
        value: String,
    },
    /// A line starting with `;` or `#`, including the marker.
    Comment(String),
    /// An empty line.
    Blank,
    /// A line that is neither a comment nor a `key=value` pair.
    Other(String),
}

/// Something in the document that prevents or complicates a safe edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IniProblem {
    /// A non-empty, non-comment line without an `=`.
    LineWithoutSeparator {
        /// One-based line number.
        line: usize,
        /// The offending text.
        text: String,
    },
    /// A key that appears more than once. Editing it is ambiguous.
    DuplicateKey {
        /// The duplicated key, trimmed.
        key: String,
        /// One-based line numbers of the occurrences.
        lines: (usize, usize),
    },
    /// A line whose key is empty, such as `=value`.
    EmptyKey {
        /// One-based line number.
        line: usize,
    },
}

impl IniProblem {
    /// One-line explanation for the UI and logs.
    pub fn summary(&self) -> String {
        match self {
            Self::LineWithoutSeparator { line, text } => {
                format!("line {line} is not a key=value pair: {text}")
            }
            Self::DuplicateKey { key, lines } => {
                format!("key `{key}` appears on lines {} and {}", lines.0, lines.1)
            }
            Self::EmptyKey { line } => format!("line {line} has an empty key"),
        }
    }
}

/// A single logical change between two documents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IniChange {
    /// The affected key.
    pub key: String,
    /// Value before the change, or `None` when the key was added.
    pub before: Option<String>,
    /// Value after the change, or `None` when the key was removed.
    pub after: Option<String>,
}

impl IniChange {
    /// Human-readable diff line.
    pub fn summary(&self) -> String {
        match (&self.before, &self.after) {
            (Some(before), Some(after)) => format!("{}: {before} -> {after}", self.key),
            (None, Some(after)) => format!("{}: (unset) -> {after}", self.key),
            (Some(before), None) => format!("{}: {before} -> (removed)", self.key),
            (None, None) => format!("{}: unchanged", self.key),
        }
    }
}

/// A parsed INI document that preserves its original shape.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IniFile {
    entries: Vec<IniEntry>,
    newline: Newline,
    trailing_newline: bool,
}

impl IniFile {
    /// An empty document.
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse a document, preserving every line.
    pub fn parse(text: &str) -> Self {
        let newline = if text.contains("\r\n") {
            Newline::CrLf
        } else {
            Newline::Lf
        };
        let trailing_newline = text.ends_with('\n');
        let body = text.strip_suffix('\n').unwrap_or(text);
        let body = body.strip_suffix('\r').unwrap_or(body);

        let mut entries = Vec::new();
        if !(body.is_empty() && !trailing_newline) {
            for raw in body.split('\n') {
                let line = raw.strip_suffix('\r').unwrap_or(raw);
                entries.push(parse_line(line));
            }
        }

        Self {
            entries,
            newline,
            trailing_newline,
        }
    }

    /// Parse a document from bytes, replacing invalid UTF-8.
    pub fn parse_bytes(bytes: &[u8]) -> Self {
        Self::parse(&String::from_utf8_lossy(bytes))
    }

    /// The line ending style that will be written back.
    pub fn newline(&self) -> Newline {
        self.newline
    }

    /// Whether the document ends with a line ending.
    pub fn has_trailing_newline(&self) -> bool {
        self.trailing_newline
    }

    /// Every line, in order.
    pub fn entries(&self) -> &[IniEntry] {
        &self.entries
    }

    /// Number of `key=value` pairs, including duplicates.
    pub fn len(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| matches!(entry, IniEntry::Pair { .. }))
            .count()
    }

    /// Whether the document has no `key=value` pairs.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Look up a key. Comparison trims surrounding whitespace on the key, which
    /// is how the Android tools treat these files. The first occurrence wins.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.iter().find_map(|entry| match entry {
            IniEntry::Pair {
                key: entry_key,
                value,
            } if entry_key.trim() == key => Some(value.as_str()),
            _ => None,
        })
    }

    /// Look up a key and trim the value.
    pub fn get_trimmed(&self, key: &str) -> Option<&str> {
        self.get(key).map(str::trim)
    }

    /// Whether a key is present, even with an empty value.
    pub fn contains(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Every key in document order, trimmed.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().filter_map(|entry| match entry {
            IniEntry::Pair { key, .. } => Some(key.trim()),
            _ => None,
        })
    }

    /// Problems that should stop or complicate an edit.
    pub fn problems(&self) -> Vec<IniProblem> {
        let mut problems = Vec::new();
        let mut seen: Vec<(&str, usize)> = Vec::new();

        for (index, entry) in self.entries.iter().enumerate() {
            let line = index + 1;
            match entry {
                IniEntry::Pair { key, .. } => {
                    let key = key.trim();
                    if key.is_empty() {
                        problems.push(IniProblem::EmptyKey { line });
                        continue;
                    }
                    if let Some((_, first)) = seen.iter().find(|(seen_key, _)| *seen_key == key) {
                        problems.push(IniProblem::DuplicateKey {
                            key: key.to_owned(),
                            lines: (*first, line),
                        });
                    } else {
                        seen.push((key, line));
                    }
                }
                IniEntry::Other(text) => {
                    problems.push(IniProblem::LineWithoutSeparator {
                        line,
                        text: text.clone(),
                    });
                }
                IniEntry::Comment(_) | IniEntry::Blank => {}
            }
        }

        problems
    }

    /// Set a key, changing as little as possible.
    ///
    /// An existing key keeps its position and its original spelling. A new key
    /// is appended after the last non-blank line so trailing blank lines stay
    /// trailing. Returns `true` when the document changed.
    pub fn set(&mut self, key: &str, value: &str) -> bool {
        let existing = self.entries.iter_mut().find_map(|entry| match entry {
            IniEntry::Pair {
                key: entry_key,
                value,
            } if entry_key.trim() == key => Some(value),
            _ => None,
        });
        if let Some(existing) = existing {
            if existing == value {
                return false;
            }
            *existing = value.to_owned();
            return true;
        }

        let insertion = self
            .entries
            .iter()
            .rposition(|entry| !matches!(entry, IniEntry::Blank))
            .map(|index| index + 1)
            .unwrap_or(0);
        self.entries.insert(
            insertion,
            IniEntry::Pair {
                key: key.to_owned(),
                value: value.to_owned(),
            },
        );
        true
    }

    /// Remove every occurrence of a key. Returns `true` when something was
    /// removed.
    pub fn remove(&mut self, key: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| match entry {
            IniEntry::Pair { key: entry_key, .. } => entry_key.trim() != key,
            _ => true,
        });
        self.entries.len() != before
    }

    /// Logical changes needed to turn `self` into `other`.
    ///
    /// Only keys whose values differ are reported, so a diff stays readable
    /// even when both documents carry hundreds of unrelated keys.
    pub fn changes_from(&self, other: &Self) -> Vec<IniChange> {
        let mut keys: Vec<&str> = self.keys().chain(other.keys()).collect();
        keys.sort_unstable();
        keys.dedup();

        keys.into_iter()
            .filter_map(|key| {
                let before = self.get_trimmed(key).map(str::to_owned);
                let after = other.get_trimmed(key).map(str::to_owned);
                (before != after).then_some(IniChange {
                    key: key.to_owned(),
                    before,
                    after,
                })
            })
            .collect()
    }

    /// Line ending style, for tests and diagnostics.
    pub fn set_newline(&mut self, newline: Newline) {
        self.newline = newline;
    }

    /// Ensure the document ends with a line ending.
    pub fn set_trailing_newline(&mut self, trailing: bool) {
        self.trailing_newline = trailing;
    }
}

fn parse_line(line: &str) -> IniEntry {
    if line.is_empty() {
        return IniEntry::Blank;
    }
    if line.starts_with(';') || line.starts_with('#') {
        return IniEntry::Comment(line.to_owned());
    }
    match line.split_once('=') {
        Some((key, value)) => IniEntry::Pair {
            key: key.to_owned(),
            value: value.to_owned(),
        },
        None => IniEntry::Other(line.to_owned()),
    }
}

impl fmt::Display for IniFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let newline = self.newline.as_str();
        for (index, entry) in self.entries.iter().enumerate() {
            if index > 0 {
                formatter.write_str(newline)?;
            }
            match entry {
                IniEntry::Pair { key, value } => write!(formatter, "{key}={value}")?,
                IniEntry::Comment(text) | IniEntry::Other(text) => formatter.write_str(text)?,
                IniEntry::Blank => {}
            }
        }
        if self.trailing_newline && !self.entries.is_empty() {
            formatter.write_str(newline)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# Managed by Avdium
AvdId=Pixel_9

hw.ramSize=8192
hw.cpu.ncore=6
unknown.future.key=keep me
";

    #[test]
    fn round_trip_is_byte_identical() {
        let ini = IniFile::parse(SAMPLE);
        assert_eq!(ini.to_string(), SAMPLE);
    }

    #[test]
    fn crlf_and_trailing_newline_are_preserved() {
        let text = "a=1\r\nb=2\r\n";
        let ini = IniFile::parse(text);
        assert_eq!(ini.newline(), Newline::CrLf);
        assert!(ini.has_trailing_newline());
        assert_eq!(ini.to_string(), text);
    }

    #[test]
    fn a_file_without_a_trailing_newline_stays_that_way() {
        let text = "a=1\nb=2";
        let ini = IniFile::parse(text);
        assert!(!ini.has_trailing_newline());
        assert_eq!(ini.to_string(), text);
    }

    #[test]
    fn an_empty_document_round_trips() {
        assert_eq!(IniFile::parse("").to_string(), "");
        assert_eq!(IniFile::parse("\n").to_string(), "\n");
    }

    #[test]
    fn values_may_contain_equals_signs() {
        let ini = IniFile::parse("hw.device.hash2=MD5:5478e3411cc0e0441240e736eb14c07a\nk=a=b\n");
        assert_eq!(ini.get("k"), Some("a=b"));
    }

    #[test]
    fn comments_and_unknown_keys_survive_an_edit() {
        let mut ini = IniFile::parse(SAMPLE);
        assert!(ini.set("hw.ramSize", "4096"));
        let text = ini.to_string();
        assert!(text.contains("# Managed by Avdium"));
        assert!(text.contains("unknown.future.key=keep me"));
        assert!(text.contains("hw.ramSize=4096"));
        // Position is preserved, not moved to the end.
        assert!(
            text.find("hw.ramSize").expect("present") < text.find("hw.cpu.ncore").expect("present")
        );
    }

    #[test]
    fn setting_an_unchanged_value_reports_no_change() {
        let mut ini = IniFile::parse(SAMPLE);
        assert!(!ini.set("hw.ramSize", "8192"));
    }

    #[test]
    fn a_new_key_is_appended_before_trailing_blanks() {
        let mut ini = IniFile::parse("a=1\n\n\n");
        assert!(ini.set("b", "2"));
        assert_eq!(ini.to_string(), "a=1\nb=2\n\n\n");
    }

    #[test]
    fn a_new_key_in_an_empty_document_does_not_add_a_leading_newline() {
        let mut ini = IniFile::parse("");
        assert!(ini.set("a", "1"));
        assert_eq!(ini.to_string(), "a=1");
    }

    #[test]
    fn removing_a_key_keeps_the_rest() {
        let mut ini = IniFile::parse(SAMPLE);
        assert!(ini.remove("hw.cpu.ncore"));
        assert!(!ini.contains("hw.cpu.ncore"));
        assert!(ini.contains("unknown.future.key"));
        assert!(!ini.remove("hw.cpu.ncore"));
    }

    #[test]
    fn duplicate_keys_are_reported_and_the_first_wins() {
        let ini = IniFile::parse("a=1\na=2\n");
        assert_eq!(ini.get("a"), Some("1"));
        let problems = ini.problems();
        assert_eq!(problems.len(), 1);
        match &problems[0] {
            IniProblem::DuplicateKey { key, lines } => {
                assert_eq!(key, "a");
                assert_eq!(*lines, (1, 2));
            }
            other => panic!("unexpected problem: {other:?}"),
        }
        assert!(problems[0].summary().contains("lines 1 and 2"));
    }

    #[test]
    fn malformed_lines_are_preserved_and_reported() {
        let text = "a=1\nthis line is not a pair\nb=2\n";
        let ini = IniFile::parse(text);
        assert_eq!(ini.to_string(), text);
        let problems = ini.problems();
        assert_eq!(problems.len(), 1);
        assert!(problems[0].summary().contains("not a key=value pair"));
    }

    #[test]
    fn an_empty_key_is_reported() {
        let ini = IniFile::parse("=value\n");
        assert_eq!(ini.problems(), vec![IniProblem::EmptyKey { line: 1 }]);
    }

    #[test]
    fn key_lookup_trims_surrounding_whitespace() {
        let ini = IniFile::parse("  hw.ramSize = 8192 \n");
        assert_eq!(ini.get("hw.ramSize"), Some(" 8192 "));
        assert_eq!(ini.get_trimmed("hw.ramSize"), Some("8192"));
    }

    #[test]
    fn diffs_report_only_changed_keys() {
        let before = IniFile::parse("a=1\nb=2\nc=3\n");
        let mut after = before.clone();
        after.set("b", "20");
        after.set("d", "4");
        after.remove("c");

        let changes = before.changes_from(&after);
        let summaries: Vec<String> = changes.iter().map(IniChange::summary).collect();
        assert_eq!(
            summaries,
            vec!["b: 2 -> 20", "c: 3 -> (removed)", "d: (unset) -> 4"]
        );
    }

    #[test]
    fn a_document_with_no_changes_produces_an_empty_diff() {
        let ini = IniFile::parse(SAMPLE);
        assert!(ini.changes_from(&ini).is_empty());
    }

    #[test]
    fn invalid_utf8_is_replaced_rather_than_rejected() {
        let bytes = b"a=1\nb=\xff\xfe\n";
        let ini = IniFile::parse_bytes(bytes);
        assert_eq!(ini.get("a"), Some("1"));
        assert!(ini.get("b").is_some());
    }

    #[test]
    fn keys_lists_in_document_order() {
        let ini = IniFile::parse("z=1\na=2\nm=3\n");
        let keys: Vec<&str> = ini.keys().collect();
        assert_eq!(keys, vec!["z", "a", "m"]);
    }
}
