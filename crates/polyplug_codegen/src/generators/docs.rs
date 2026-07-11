//! Shared escaping primitives for generated contract documentation.

/// Return documentation as normalized logical lines.
pub(crate) fn lines(docs: Option<&str>) -> Vec<&str> {
    docs.map(|docs| docs.split('\n').collect())
        .unwrap_or_default()
}

/// Append optional user documentation after the generator's built-in item summary.
pub(crate) fn append_lines(target: &mut Vec<String>, docs: Option<&str>) {
    target.extend(lines(docs).into_iter().map(str::to_owned));
}

/// Write a safe TypeScript JSDoc block.
pub(crate) fn write_jsdoc(
    out: &mut String,
    indent: &str,
    docs: Option<&str>,
    params: &[(&str, Option<&str>)],
    returns: Option<&str>,
) {
    if docs.is_none() && params.iter().all(|(_, docs)| docs.is_none()) && returns.is_none() {
        return;
    }

    out.push_str(indent);
    out.push_str("/**\n");
    for line in lines(docs) {
        out.push_str(indent);
        out.push_str(" * ");
        out.push_str(&line.replace("*/", "*\\/"));
        out.push('\n');
    }
    for (name, docs) in params {
        if let Some(docs) = docs {
            for (index, line) in lines(Some(docs)).into_iter().enumerate() {
                out.push_str(indent);
                if index == 0 {
                    out.push_str(&format!(" * @param {name} "));
                } else {
                    out.push_str(" *        ");
                }
                out.push_str(&line.replace("*/", "*\\/"));
                out.push('\n');
            }
        }
    }
    if let Some(returns) = returns {
        for (index, line) in lines(Some(returns)).into_iter().enumerate() {
            out.push_str(indent);
            if index == 0 {
                out.push_str(" * @returns ");
            } else {
                out.push_str(" *          ");
            }
            out.push_str(&line.replace("*/", "*\\/"));
            out.push('\n');
        }
    }
    out.push_str(indent);
    out.push_str(" */\n");
}

/// Write a Python triple-quoted docstring. Escaping preserves literal backslashes,
/// physical newlines, and prevents a documentation value from terminating its own
/// string literal.
pub(crate) fn write_python_docstring(out: &mut String, indent: &str, docs: Option<&str>) {
    let Some(docs) = docs else {
        return;
    };
    out.push_str(indent);
    out.push_str(&python_docstring_literal(docs));
    out.push('\n');
}

/// Render documentation as a Python triple-quoted string literal.
pub(crate) fn python_docstring_literal(docs: &str) -> String {
    format!(
        "\"\"\"{}\"\"\"",
        docs.replace('\\', "\\\\")
            .replace('\n', "\\n")
            .replace("\"\"\"", "\\\"\"\"")
    )
}

pub(crate) fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\"', "&quot;")
        .replace('\'', "&apos;")
}

/// Write C# XML documentation with text escaped as XML character data.
pub(crate) fn write_csharp_xml_docs(
    out: &mut String,
    indent: &str,
    docs: Option<&str>,
    params: &[(&str, Option<&str>)],
    returns: Option<&str>,
) {
    if docs.is_none() && params.iter().all(|(_, docs)| docs.is_none()) && returns.is_none() {
        return;
    }
    out.push_str(indent);
    out.push_str("/// <summary>\n");
    for line in lines(docs) {
        out.push_str(indent);
        out.push_str("/// ");
        out.push_str(&xml_escape(line));
        out.push('\n');
    }
    out.push_str(indent);
    out.push_str("/// </summary>\n");
    for (name, docs) in params {
        if let Some(docs) = docs {
            for (index, line) in lines(Some(docs)).into_iter().enumerate() {
                out.push_str(indent);
                if index == 0 {
                    out.push_str(&format!("/// <param name=\"{name}\">"));
                } else {
                    out.push_str("/// ");
                }
                out.push_str(&xml_escape(line));
                out.push('\n');
            }
            out.push_str(indent);
            out.push_str("/// </param>\n");
        }
    }
    if let Some(returns) = returns {
        out.push_str(indent);
        out.push_str("/// <returns>\n");
        for line in lines(Some(returns)) {
            out.push_str(indent);
            out.push_str("/// ");
            out.push_str(&xml_escape(line));
            out.push('\n');
        }
        out.push_str(indent);
        out.push_str("/// </returns>\n");
    }
}

/// Write LuaLS/EmmyLua documentation lines.
pub(crate) fn write_luals_docs(out: &mut String, indent: &str, docs: Option<&str>) {
    for line in lines(docs) {
        out.push_str(indent);
        out.push_str("--- ");
        out.push_str(line);
        out.push('\n');
    }
}
