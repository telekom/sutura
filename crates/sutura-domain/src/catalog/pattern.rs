//! Bounded, case-sensitive dictionary identifier patterns. Only `*` and `?` are special.

/// An identifier pattern, with no path, regex or SQL syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryPattern(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidDictionaryPattern {
    #[error("a dictionary pattern must contain 1 through 128 bytes without control characters")]
    Shape,
}

impl DictionaryPattern {
    pub fn parse(text: &str) -> Result<Self, InvalidDictionaryPattern> {
        if text.is_empty() || text.len() > 128 || text.chars().any(char::is_control) {
            return Err(InvalidDictionaryPattern::Shape);
        }
        Ok(Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Bounded dynamic programming: at most 128 pattern bytes by 256 candidate bytes.
    /// Dictionary identifiers beyond the runtime's identifier limit are refused separately.
    pub fn matches(&self, text: &str) -> bool {
        if text.len() > 256 {
            return false;
        }
        let mut states = vec![false; text.chars().count() + 1];
        if let Some(first) = states.first_mut() {
            *first = true;
        }
        for pattern in self.0.chars() {
            if pattern == '*' {
                let mut reached = false;
                for state in &mut states {
                    reached |= *state;
                    *state = reached;
                }
            } else if let Some((first, rest)) = states.split_first_mut() {
                let mut previous = *first;
                *first = false;
                for (state, character) in rest.iter_mut().zip(text.chars()) {
                    let old = *state;
                    *state = previous && (pattern == '?' || pattern == character);
                    previous = old;
                }
            }
        }
        states.last().copied().unwrap_or(false)
    }

    /// A bound SQL LIKE value used only to prefilter schemas. The matcher remains authoritative.
    pub fn sql_like(&self) -> String {
        let mut value = String::new();
        for character in self.0.chars() {
            match character {
                '*' => value.push('%'),
                '?' => value.push('_'),
                '%' | '_' | '\\' => {
                    value.push('\\');
                    value.push(character);
                }
                literal => value.push(literal),
            }
        }
        value
    }
}

/// One schema/table include pair. Matching follows the dictionary's exact returned case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryInclude {
    schema: DictionaryPattern,
    table: DictionaryPattern,
}

impl DictionaryInclude {
    pub const fn new(schema: DictionaryPattern, table: DictionaryPattern) -> Self {
        Self { schema, table }
    }
    pub const fn schema(&self) -> &DictionaryPattern {
        &self.schema
    }
    pub const fn table(&self) -> &DictionaryPattern {
        &self.table
    }
}

/// Native dictionary selection. Counts bound matching work independently of metadata read bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionarySelection {
    include: Vec<DictionaryInclude>,
    exclude_tables: Vec<DictionaryPattern>,
    exclude_columns: Vec<DictionaryPattern>,
    exclude_types: Vec<DictionaryPattern>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidDictionarySelection {
    #[error("native dictionary selection needs 1 through 64 include pairs and at most 128 exclusion patterns")]
    Count,
}

impl DictionarySelection {
    pub fn parse(
        include: Vec<DictionaryInclude>,
        exclude_tables: Vec<DictionaryPattern>,
        exclude_columns: Vec<DictionaryPattern>,
        exclude_types: Vec<DictionaryPattern>,
    ) -> Result<Self, InvalidDictionarySelection> {
        if include.is_empty()
            || include.len() > 64
            || exclude_tables
                .len()
                .saturating_add(exclude_columns.len())
                .saturating_add(exclude_types.len())
                > 128
        {
            return Err(InvalidDictionarySelection::Count);
        }
        Ok(Self {
            include,
            exclude_tables,
            exclude_columns,
            exclude_types,
        })
    }
    pub fn include(&self) -> &[DictionaryInclude] {
        &self.include
    }
    pub fn excludes_table(&self, table: &str) -> bool {
        self.exclude_tables.iter().any(|pattern| pattern.matches(table))
    }
    pub fn excludes_column(&self, column: &str) -> bool {
        self.exclude_columns.iter().any(|pattern| pattern.matches(column))
    }
    pub fn excludes_type(&self, native: &str) -> bool {
        let base = native.split('(').next().unwrap_or(native);
        self.exclude_types
            .iter()
            .any(|pattern| pattern.matches(native) || pattern.matches(base))
    }
}
