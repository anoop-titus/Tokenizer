use regex::Regex;
use std::sync::LazyLock;

use super::scanner::FileCategory;

/// Compression level: 1=light, 2=medium, 3=heavy, 4=maximum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompressionLevel(pub u8);

impl CompressionLevel {
    pub fn clamp(level: u8) -> Self {
        Self(level.clamp(1, 4))
    }
}

static ABBREVIATIONS: LazyLock<Vec<(&str, &str)>> = LazyLock::new(|| {
    vec![
        ("configuration", "cfg"),
        ("service", "svc"),
        ("directory", "dir"),
        ("environment", "env"),
        ("authentication", "auth"),
        ("repository", "repo"),
        ("command", "cmd"),
        ("function", "fn"),
        ("required", "req"),
        ("optional", "opt"),
        ("context", "ctx"),
        ("system", "sys"),
        ("management", "mgmt"),
        ("operations", "ops"),
        ("infrastructure", "infra"),
        ("integration", "int"),
        ("execution", "exec"),
        ("description", "desc"),
        ("implementation", "impl"),
        ("value", "val"),
        ("default", "def"),
        ("session", "sess"),
        ("project", "proj"),
        ("workspace", "wksp"),
        ("template", "tpl"),
        ("monitor", "mon"),
        ("schedule", "sched"),
    ]
});

static ABBREV_REGEXES: LazyLock<Vec<(Regex, &str)>> = LazyLock::new(|| {
    ABBREVIATIONS
        .iter()
        .map(|(long, short)| {
            let pattern = format!(r"(?i)\b{}\b", regex::escape(long));
            (
                Regex::new(&pattern).expect("valid abbreviation regex"),
                *short,
            )
        })
        .collect()
});

static BOLD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\*\*(.+?)\*\*").expect("bold regex"));
static ITALIC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\*(.+?)\*").expect("italic regex"));
static HR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-{3,}$").expect("hr regex"));
static EMOJI_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[\p{Emoji_Presentation}\p{Emoji}\x{FE0F}\x{200D}]+").expect("emoji regex")
});
static FILLER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(please note that|it is important to note that|as mentioned above|in order to|basically|essentially|simply put)\b").expect("filler regex")
});

fn apply_abbreviations(text: &str) -> String {
    let mut result = text.to_string();
    for (re, short) in ABBREV_REGEXES.iter() {
        result = re.replace_all(&result, *short).to_string();
    }
    result
}

/// Convert markdown content to .toon format.
/// Ports the Python token-police.py logic to Rust.
pub fn convert_md_to_toon(
    content: &str,
    level: CompressionLevel,
    category: FileCategory,
) -> String {
    // Detect and preserve YAML frontmatter
    let (frontmatter, body) = if let Some(rest) = content.strip_prefix("---") {
        if let Some(end_pos) = rest.find("---") {
            let fm = format!("---{}---\n", &rest[..end_pos]);
            let body = &rest[end_pos + 3..];
            (Some(fm), body)
        } else {
            (None, content)
        }
    } else {
        (None, content)
    };

    let lines = body.split('\n');
    let mut out: Vec<String> = Vec::new();
    let mut in_code_block = false;
    let mut in_table = false;
    let mut table_headers: Vec<String> = Vec::new();
    let mut table_rows: Vec<String> = Vec::new();

    for raw_line in lines {
        let stripped = raw_line.trim();

        // Level >= 2: skip horizontal rules
        if level.0 >= 2 && HR_RE.is_match(stripped) {
            continue;
        }

        // Level >= 2: skip empty lines
        if level.0 >= 2 && stripped.is_empty() {
            continue;
        }

        // Level 1: keep blank lines (lighter touch)
        if level.0 == 1 && stripped.is_empty() {
            out.push(String::new());
            continue;
        }

        // Handle code blocks
        if stripped.starts_with("```") {
            if in_code_block {
                in_code_block = false;
                continue;
            } else {
                in_code_block = true;
                continue;
            }
        }

        if in_code_block {
            if level.0 >= 2 {
                out.push(format!("cmd`{}`", stripped));
            } else {
                out.push(raw_line.to_string());
            }
            continue;
        }

        // Level 4: tables to JSONL
        if level.0 >= 4 && stripped.contains('|') && !stripped.starts_with('#') {
            let cells: Vec<String> = stripped
                .split('|')
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty())
                .collect();

            // Skip separator rows
            if cells.iter().all(|c| c.chars().all(|ch| ch == '-')) {
                continue;
            }

            if !in_table {
                in_table = true;
                table_headers = cells;
                continue;
            } else {
                let mut row = serde_json::Map::new();
                for (i, h) in table_headers.iter().enumerate() {
                    if i < cells.len() {
                        row.insert(h.clone(), serde_json::Value::String(cells[i].clone()));
                    }
                }
                table_rows.push(serde_json::to_string(&row).unwrap_or_default());
                continue;
            }
        }

        // Flush table if we left it
        if in_table && !stripped.contains('|') {
            out.append(&mut table_rows);
            table_headers.clear();
            in_table = false;
        }

        let mut line = raw_line.to_string();

        // Level >= 2: headers to >>section
        if level.0 >= 2 {
            if let Some(rest) = stripped.strip_prefix("### ") {
                out.push(format!(">>{}", rest.to_lowercase().replace(' ', "_")));
                continue;
            }
            if let Some(rest) = stripped.strip_prefix("## ") {
                out.push(format!(">>{}", rest.to_lowercase().replace(' ', "_")));
                continue;
            }
            if let Some(rest) = stripped.strip_prefix("# ") {
                out.push(format!(">>{}", rest.to_lowercase().replace(' ', "_")));
                continue;
            }
        }

        // Level >= 2: strip bold and italic
        if level.0 >= 2 {
            line = BOLD_RE.replace_all(&line, "$1").to_string();
            line = ITALIC_RE.replace_all(&line, "$1").to_string();
        }

        // Level >= 3: strip emoji
        if level.0 >= 3 {
            line = EMOJI_RE.replace_all(&line, "").to_string();
        }

        // Level >= 3: strip filler prose
        if level.0 >= 3 {
            line = FILLER_RE.replace_all(&line, "").to_string();
        }

        // Level >= 3: flatten nested lists (reduce indent)
        if level.0 >= 3 && stripped.starts_with("- ") {
            line = format!("- {}", stripped.trim_start_matches("- "));
        }

        out.push(line.trim_end().to_string());
    }

    // Flush remaining table
    if in_table && !table_rows.is_empty() {
        out.extend(table_rows);
    }

    let mut result = out.join("\n");

    // All levels: apply abbreviations
    result = apply_abbreviations(&result);

    // Prepend type header
    let type_label = match category {
        FileCategory::Agent => "agent",
        FileCategory::Rule => "rule",
        FileCategory::Skill => "skill",
        FileCategory::Memory => "memory",
        FileCategory::Command => "command",
        FileCategory::TopLevel => "config",
        FileCategory::Whitelisted => "config",
    };

    let mut final_output = String::new();
    if let Some(fm) = frontmatter {
        final_output.push_str(&fm);
    }
    final_output.push_str(&format!("@type:{type_label}\n"));
    final_output.push_str(&result);
    final_output.push('\n');
    final_output
}

/// Return valid target formats for a one-way optimization pipeline.
/// md -> json/jsonl/toon, json/jsonl -> toon, toon -> nothing.
#[allow(dead_code)]
pub fn valid_targets(current_format: &str) -> Vec<&'static str> {
    match current_format {
        "md" => vec!["json", "jsonl", "toon"],
        "json" | "jsonl" => vec!["toon"],
        _ => vec![],
    }
}

/// Convert markdown content to a structured JSON intermediate format.
#[allow(dead_code)]
pub fn convert_md_to_json(content: &str, category: FileCategory) -> String {
    let (frontmatter, body) = if let Some(rest) = content.strip_prefix("---") {
        if let Some(end_pos) = rest.find("---") {
            let fm = rest[..end_pos].trim().to_string();
            let body = rest[end_pos + 3..].trim_start_matches('\n').to_string();
            (Some(fm), body)
        } else {
            (None, content.to_string())
        }
    } else {
        (None, content.to_string())
    };

    let type_label = match category {
        FileCategory::Agent => "agent",
        FileCategory::Rule => "rule",
        FileCategory::Skill => "skill",
        FileCategory::Memory => "memory",
        FileCategory::Command => "command",
        FileCategory::TopLevel => "config",
        FileCategory::Whitelisted => "config",
    };

    let abbreviated_body = apply_abbreviations(&body);

    let fm_value = match frontmatter {
        Some(fm) => serde_json::Value::String(fm),
        None => serde_json::Value::Null,
    };

    let obj = serde_json::json!({
        "type": type_label,
        "frontmatter": fm_value,
        "body": abbreviated_body,
    });

    serde_json::to_string_pretty(&obj).unwrap_or_default()
}

/// Convert JSON intermediate format to .toon format.
#[allow(dead_code)]
pub fn convert_json_to_toon(content: &str) -> String {
    let parsed: serde_json::Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => return content.to_string(),
    };

    let type_label = parsed
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("config");
    let body = parsed.get("body").and_then(|v| v.as_str()).unwrap_or("");

    let mut out = Vec::new();
    for line in body.lines() {
        let stripped = line.trim();
        // Convert markdown headers to >>section
        if let Some(rest) = stripped.strip_prefix("### ") {
            out.push(format!(">>{}", rest.to_lowercase().replace(' ', "_")));
        } else if let Some(rest) = stripped.strip_prefix("## ") {
            out.push(format!(">>{}", rest.to_lowercase().replace(' ', "_")));
        } else if let Some(rest) = stripped.strip_prefix("# ") {
            out.push(format!(">>{}", rest.to_lowercase().replace(' ', "_")));
        } else {
            // Strip bold/italic markdown
            let cleaned = BOLD_RE.replace_all(stripped, "$1");
            let cleaned = ITALIC_RE.replace_all(&cleaned, "$1");
            out.push(cleaned.to_string());
        }
    }

    let toon_body = apply_abbreviations(&out.join("\n"));

    let mut result = format!("@type:{type_label}\n");
    result.push_str(&toon_body);
    result.push('\n');
    result
}

/// Estimate converted size without actually writing. Used for before/after preview.
#[allow(dead_code)]
pub fn estimate_converted_size(
    content: &str,
    level: CompressionLevel,
    category: FileCategory,
) -> usize {
    convert_md_to_toon(content, level, category).len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_abbreviations() {
        let input = "The configuration of the service directory";
        let result = apply_abbreviations(input);
        assert!(result.contains("cfg"));
        assert!(result.contains("svc"));
        assert!(result.contains("dir"));
    }

    #[test]
    fn test_light_compression() {
        let input = "# Header\n\nSome **bold** text\n\n- item\n";
        let result = convert_md_to_toon(input, CompressionLevel(1), FileCategory::Rule);
        // Level 1: keeps headers as-is, keeps bold, keeps blanks
        assert!(result.contains("# Header"));
        assert!(result.contains("**bold**"));
    }

    #[test]
    fn test_medium_compression() {
        let input = "# Header\n\nSome **bold** text\n\n---\n\n```bash\necho hello\n```\n";
        let result = convert_md_to_toon(input, CompressionLevel(2), FileCategory::Rule);
        assert!(result.contains(">>header"));
        assert!(!result.contains("**bold**"));
        assert!(result.contains("cmd`echo hello`"));
    }

    #[test]
    fn test_frontmatter_preserved() {
        let input = "---\ntitle: test\n---\n# Hello\n";
        let result = convert_md_to_toon(input, CompressionLevel(2), FileCategory::Memory);
        assert!(result.starts_with("---\ntitle: test\n---\n"));
    }
}
