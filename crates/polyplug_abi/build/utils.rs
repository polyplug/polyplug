/// Indent a string by the specified number of spaces.
///
/// # Arguments
/// * `s` - The string to indent
/// * `spaces` - Number of spaces to indent each line
///
/// # Returns
/// A new string with each line indented.
pub fn indent(s: &str, spaces: usize) -> String {
    let indent_str: String = " ".repeat(spaces);
    s.lines()
        .map(|line: &str| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{}{}", indent_str, line)
            }
        })
        .collect::<Vec<String>>()
        .join("\n")
}

/// Convert a snake_case identifier to PascalCase.
///
/// # Arguments
/// * `s` - The snake_case string to convert
///
/// # Returns
/// A PascalCase string.
pub fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word: &str| {
            let mut chars: core::str::Chars<'_> = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    let first_upper: String = first.to_uppercase().collect();
                    let rest_lower: String = chars.as_str().to_lowercase();
                    format!("{}{}", first_upper, rest_lower)
                }
            }
        })
        .collect()
}

/// Convert a PascalCase identifier to snake_case.
///
/// # Arguments
/// * `s` - The PascalCase string to convert
///
/// # Returns
/// A snake_case string.
pub fn to_snake_case(s: &str) -> String {
    let mut result: String = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c.to_lowercase().next().unwrap_or(c));
        } else {
            result.push(c);
        }
    }
    result
}

/// Generate a documentation comment for the target language.
///
/// # Arguments
/// * `doc` - The documentation text
/// * `prefix` - The comment prefix (e.g., "///", "//", "#")
///
/// # Returns
/// A formatted documentation comment string.
pub fn format_doc_comment(doc: &str, prefix: &str) -> String {
    doc.lines()
        .map(|line: &str| format!("{} {}", prefix, line))
        .collect::<Vec<String>>()
        .join("\n")
}
