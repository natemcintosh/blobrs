//! Preview module for displaying CSV, TSV, JSON, and text file contents.

use std::io::Cursor;

/// Maximum bytes to download for preview (50KB)
pub const MAX_PREVIEW_BYTES: usize = 50 * 1024;

/// Maximum rows to display in table preview
pub const MAX_PREVIEW_ROWS: usize = 100;

/// Supported file types for preview
#[derive(Debug, Clone, PartialEq)]
pub enum PreviewFileType {
    Csv,
    Tsv,
    Json,
    /// Generic text file with extension name (e.g., "MD", "PY", "TXT")
    Text(String),
    Unsupported,
}

/// Known text file extensions (lowercase, without the dot)
const TEXT_EXTENSIONS: &[&str] = &[
    // Documentation
    "txt",
    "md",
    "markdown",
    "rst",
    "adoc",
    "asciidoc",
    // Config files
    "yaml",
    "yml",
    "toml",
    "ini",
    "cfg",
    "conf",
    "config",
    // Data/markup
    "xml",
    "html",
    "htm",
    "svg",
    "css",
    // Scripts/code
    "sh",
    "bash",
    "zsh",
    "fish",
    "ps1",
    "py",
    "pyw",
    "pyi",
    "rs",
    "go",
    "rb",
    "pl",
    "lua",
    "js",
    "ts",
    "jsx",
    "tsx",
    "mjs",
    "cjs",
    "c",
    "h",
    "cpp",
    "hpp",
    "cc",
    "cxx",
    "java",
    "kt",
    "kts",
    "scala",
    "groovy",
    "cs",
    "fs",
    "vb",
    "swift",
    "m",
    "mm",
    "r",
    "jl",
    "ex",
    "exs",
    "erl",
    "hrl",
    "hs",
    "lhs",
    "ml",
    "mli",
    "clj",
    "cljs",
    "sql",
    "graphql",
    "gql",
    // Build/project files
    "makefile",
    "cmake",
    "dockerfile",
    "gradle",
    "sbt",
    "cabal",
    // Misc
    "log",
    "env",
    "gitignore",
    "gitattributes",
    "editorconfig",
    "prettierrc",
    "eslintrc",
    "lock", // e.g., Cargo.lock, package-lock
];

impl PreviewFileType {
    /// Detect file type from file extension
    pub fn from_extension(filename: &str) -> Self {
        let lower = filename.to_lowercase();
        if lower.ends_with(".csv") {
            Self::Csv
        } else if lower.ends_with(".tsv") || lower.ends_with(".tab") {
            Self::Tsv
        } else if lower.ends_with(".json") || lower.ends_with(".jsonl") {
            Self::Json
        } else if let Some(ext) = Self::extract_extension(&lower) {
            if TEXT_EXTENSIONS.contains(&ext.as_str()) {
                Self::Text(ext.to_uppercase())
            } else {
                Self::Unsupported
            }
        } else {
            // No extension - check for known extensionless files
            let basename = filename
                .rsplit('/')
                .next()
                .unwrap_or(filename)
                .to_lowercase();
            if matches!(
                basename.as_str(),
                "makefile" | "dockerfile" | "gemfile" | "rakefile" | "justfile" | "procfile"
            ) {
                Self::Text(basename.to_uppercase())
            } else {
                Self::Unsupported
            }
        }
    }

    /// Try to detect if data is valid UTF-8 text (for unknown extensions)
    pub fn detect_from_content(data: &[u8]) -> Self {
        // Check for common binary signatures
        if is_likely_binary(data) {
            return Self::Unsupported;
        }

        // Check if the content is valid UTF-8
        if std::str::from_utf8(data).is_ok() {
            Self::Text("TEXT".to_string())
        } else {
            Self::Unsupported
        }
    }

    /// Extract file extension (without the dot)
    fn extract_extension(filename: &str) -> Option<String> {
        let basename = filename.rsplit('/').next().unwrap_or(filename);
        if let Some(dot_pos) = basename.rfind('.') {
            let ext = &basename[dot_pos + 1..];
            if !ext.is_empty() {
                return Some(ext.to_string());
            }
        }
        None
    }

    /// Check if this file type is supported for preview
    pub fn is_supported(&self) -> bool {
        !matches!(self, Self::Unsupported)
    }

    /// Get the display name for this file type
    pub fn display_name(&self) -> String {
        match self {
            Self::Csv => "CSV".to_string(),
            Self::Tsv => "TSV".to_string(),
            Self::Json => "JSON".to_string(),
            Self::Text(ext) => ext.clone(),
            Self::Unsupported => "Unsupported".to_string(),
        }
    }
}

/// Represents the data to be previewed
#[derive(Debug, Clone)]
pub enum PreviewData {
    /// Tabular data (CSV, TSV, or JSON array of objects)
    Table(TablePreview),
    /// Pretty-printed JSON (for non-array JSON)
    Json(JsonPreview),
    /// Generic text file
    Text(TextPreview),
}

/// Tabular preview data
#[derive(Debug, Clone)]
pub struct TablePreview {
    /// Column headers (if available)
    pub headers: Vec<String>,
    /// Data rows (each row is a vector of cell values)
    pub rows: Vec<Vec<String>>,
    /// Total row count (may be more than displayed rows)
    pub total_rows: usize,
    /// Whether the data was truncated due to size limits
    pub truncated: bool,
    /// File type that produced this table
    pub file_type: PreviewFileType,
}

/// JSON preview data (for non-tabular JSON)
#[derive(Debug, Clone)]
pub struct JsonPreview {
    /// Pretty-printed JSON string (or raw content if parsing failed)
    pub content: String,
    /// Whether the content was truncated
    pub truncated: bool,
    /// Total lines in the formatted output
    pub total_lines: usize,
    /// Whether this is raw content (parsing failed, likely due to truncation)
    pub is_raw: bool,
}

/// Text preview data (for generic text files)
#[derive(Debug, Clone)]
pub struct TextPreview {
    /// Text content
    pub content: String,
    /// Whether the content was truncated due to size limits
    pub truncated: bool,
    /// Total lines in the content
    pub total_lines: usize,
    /// File extension (for display purposes)
    pub extension: String,
}

/// Parse CSV data from bytes
pub fn parse_csv(data: &[u8]) -> Result<PreviewData, String> {
    parse_delimited(data, b',', PreviewFileType::Csv)
}

/// Parse TSV data from bytes
pub fn parse_tsv(data: &[u8]) -> Result<PreviewData, String> {
    parse_delimited(data, b'\t', PreviewFileType::Tsv)
}

/// Parse delimited data (CSV or TSV)
fn parse_delimited(
    data: &[u8],
    delimiter: u8,
    file_type: PreviewFileType,
) -> Result<PreviewData, String> {
    let cursor = Cursor::new(data);
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true) // Allow varying number of fields
        .has_headers(true)
        .from_reader(cursor);

    // Get headers
    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| format!("Failed to read headers: {e}"))?
        .iter()
        .map(String::from)
        .collect();

    // Read rows
    let mut rows = Vec::new();
    let mut total_rows = 0;
    let mut truncated = false;

    for result in reader.records() {
        total_rows += 1;
        if rows.len() >= MAX_PREVIEW_ROWS {
            truncated = true;
            continue; // Count remaining rows but don't store them
        }

        match result {
            Ok(record) => {
                let row: Vec<String> = record.iter().map(String::from).collect();
                rows.push(row);
            }
            Err(e) => {
                // Log error but continue with partial data
                if rows.is_empty() {
                    return Err(format!("Failed to parse data: {e}"));
                }
                truncated = true;
                break;
            }
        }
    }

    Ok(PreviewData::Table(TablePreview {
        headers,
        rows,
        total_rows,
        truncated,
        file_type,
    }))
}

/// Parse JSON data from bytes
pub fn parse_json(data: &[u8]) -> Result<PreviewData, String> {
    // First, ensure we have valid UTF-8 (truncation might cut mid-character)
    let text = match std::str::from_utf8(data) {
        Ok(s) => s.to_string(),
        Err(_) => {
            // Try to find valid UTF-8 by trimming from the end
            let valid_text = make_valid_utf8(data);
            if valid_text.is_empty() {
                return Err("Could not decode file as UTF-8".to_string());
            }
            valid_text
        }
    };

    // Try to parse as JSON
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(value) => {
            // Check if it's an array of objects (can be displayed as table)
            if let serde_json::Value::Array(arr) = &value
                && let Some(table) = try_json_array_as_table(arr)
            {
                return Ok(PreviewData::Table(table));
            }

            // Fall back to pretty-printed JSON
            let pretty = serde_json::to_string_pretty(&value)
                .map_err(|e| format!("Failed to format JSON: {e}"))?;

            let total_lines = pretty.lines().count();
            let truncated = total_lines > MAX_PREVIEW_ROWS * 2; // Allow more lines for JSON

            // Truncate if too long
            let content = if truncated {
                pretty
                    .lines()
                    .take(MAX_PREVIEW_ROWS * 2)
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                pretty
            };

            Ok(PreviewData::Json(JsonPreview {
                content,
                truncated,
                total_lines,
                is_raw: false,
            }))
        }
        Err(_) => {
            // JSON parsing failed (likely truncated) - show raw content
            let total_lines = text.lines().count();
            let truncated = true; // We assume it's truncated since parsing failed

            // Truncate display if too long
            let content = if total_lines > MAX_PREVIEW_ROWS * 2 {
                text.lines()
                    .take(MAX_PREVIEW_ROWS * 2)
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                text
            };

            Ok(PreviewData::Json(JsonPreview {
                content,
                truncated,
                total_lines,
                is_raw: true,
            }))
        }
    }
}

/// Convert bytes to valid UTF-8 by trimming invalid bytes from the end
fn make_valid_utf8(data: &[u8]) -> String {
    // Try progressively shorter slices until we get valid UTF-8
    // (truncation might have cut in the middle of a multi-byte character)
    for trim in 0..4 {
        if data.len() <= trim {
            break;
        }
        if let Ok(s) = std::str::from_utf8(&data[..data.len() - trim]) {
            return s.to_string();
        }
    }
    // If still invalid, use lossy conversion
    String::from_utf8_lossy(data).to_string()
}

/// Check if data is likely a binary file (not text)
fn is_likely_binary(data: &[u8]) -> bool {
    // Check first few KB for binary indicators
    let check_len = data.len().min(8192);
    let sample = &data[..check_len];

    // Count null bytes and other control characters
    let mut null_count = 0;
    let mut control_count = 0;

    for &byte in sample {
        if byte == 0 {
            null_count += 1;
        } else if byte < 0x09 || (byte > 0x0D && byte < 0x20 && byte != 0x1B) {
            // Control chars except tab, newline, carriage return, form feed, escape
            control_count += 1;
        }
    }

    // If there are null bytes, it's almost certainly binary
    if null_count > 0 {
        return true;
    }

    // If more than 10% are unusual control characters, likely binary
    if control_count > check_len / 10 {
        return true;
    }

    false
}

/// Try to convert a JSON array to a table (if it's an array of objects with consistent keys)
fn try_json_array_as_table(arr: &[serde_json::Value]) -> Option<TablePreview> {
    if arr.is_empty() {
        return None;
    }

    // Check if first element is an object
    let first_obj = arr.first()?.as_object()?;

    // Get headers from first object's keys
    let headers: Vec<String> = first_obj.keys().cloned().collect();

    if headers.is_empty() {
        return None;
    }

    // Convert each object to a row
    let mut rows = Vec::new();
    let total_rows = arr.len();
    let truncated = total_rows > MAX_PREVIEW_ROWS;

    for (i, item) in arr.iter().enumerate() {
        if i >= MAX_PREVIEW_ROWS {
            break;
        }

        if let serde_json::Value::Object(obj) = item {
            let row: Vec<String> = headers
                .iter()
                .map(|key| obj.get(key).map(value_to_string).unwrap_or_default())
                .collect();
            rows.push(row);
        } else {
            // Not an object, can't use table format
            return None;
        }
    }

    Some(TablePreview {
        headers,
        rows,
        total_rows,
        truncated,
        file_type: PreviewFileType::Json,
    })
}

/// Convert a JSON value to a display string
fn value_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => {
            // Show abbreviated array
            if arr.len() <= 3 {
                format!(
                    "[{}]",
                    arr.iter()
                        .map(value_to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                format!("[{} items]", arr.len())
            }
        }
        serde_json::Value::Object(obj) => {
            // Show abbreviated object
            if obj.len() <= 2 {
                format!(
                    "{{{}}}",
                    obj.iter()
                        .map(|(k, v)| format!("{k}: {}", value_to_string(v)))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                format!("{{{} keys}}", obj.len())
            }
        }
    }
}

/// Parse data based on file type
pub fn parse_preview(data: &[u8], file_type: &PreviewFileType) -> Result<PreviewData, String> {
    match file_type {
        PreviewFileType::Csv => parse_csv(data),
        PreviewFileType::Tsv => parse_tsv(data),
        PreviewFileType::Json => parse_json(data),
        PreviewFileType::Text(ext) => parse_text(data, ext),
        PreviewFileType::Unsupported => {
            // Last resort: try to detect if it's valid UTF-8 text
            let detected = PreviewFileType::detect_from_content(data);
            if let PreviewFileType::Text(ext) = detected {
                parse_text(data, &ext)
            } else {
                Err("Cannot preview binary file".to_string())
            }
        }
    }
}

/// Parse text data from bytes
pub fn parse_text(data: &[u8], extension: &str) -> Result<PreviewData, String> {
    // Ensure we have valid UTF-8
    let text = match std::str::from_utf8(data) {
        Ok(s) => s.to_string(),
        Err(_) => {
            // Try to salvage valid UTF-8 by trimming from the end
            let valid_text = make_valid_utf8(data);
            if valid_text.is_empty() {
                return Err("Cannot preview binary file".to_string());
            }
            valid_text
        }
    };

    let total_lines = text.lines().count();
    // Check if we likely have truncated content (hit the 50KB limit)
    let truncated = data.len() >= MAX_PREVIEW_BYTES;

    // Truncate display if too many lines
    let content = if total_lines > MAX_PREVIEW_ROWS * 2 {
        text.lines()
            .take(MAX_PREVIEW_ROWS * 2)
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        text
    };

    Ok(PreviewData::Text(TextPreview {
        content,
        truncated,
        total_lines,
        extension: extension.to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_type_detection() {
        assert_eq!(
            PreviewFileType::from_extension("data.csv"),
            PreviewFileType::Csv
        );
        assert_eq!(
            PreviewFileType::from_extension("data.CSV"),
            PreviewFileType::Csv
        );
        assert_eq!(
            PreviewFileType::from_extension("data.tsv"),
            PreviewFileType::Tsv
        );
        assert_eq!(
            PreviewFileType::from_extension("data.tab"),
            PreviewFileType::Tsv
        );
        assert_eq!(
            PreviewFileType::from_extension("data.json"),
            PreviewFileType::Json
        );
        // Text file extensions
        assert_eq!(
            PreviewFileType::from_extension("readme.txt"),
            PreviewFileType::Text("TXT".to_string())
        );
        assert_eq!(
            PreviewFileType::from_extension("README.md"),
            PreviewFileType::Text("MD".to_string())
        );
        assert_eq!(
            PreviewFileType::from_extension("script.py"),
            PreviewFileType::Text("PY".to_string())
        );
        assert_eq!(
            PreviewFileType::from_extension("config.yaml"),
            PreviewFileType::Text("YAML".to_string())
        );
        // Extensionless known files
        assert_eq!(
            PreviewFileType::from_extension("Makefile"),
            PreviewFileType::Text("MAKEFILE".to_string())
        );
        assert_eq!(
            PreviewFileType::from_extension("Dockerfile"),
            PreviewFileType::Text("DOCKERFILE".to_string())
        );
        // Unknown extension
        assert_eq!(
            PreviewFileType::from_extension("data.xyz"),
            PreviewFileType::Unsupported
        );
    }

    #[test]
    fn test_csv_parsing() {
        let csv_data = b"name,age,city\nAlice,30,NYC\nBob,25,LA";
        let result = parse_csv(csv_data).unwrap();

        if let PreviewData::Table(table) = result {
            assert_eq!(table.headers, vec!["name", "age", "city"]);
            assert_eq!(table.rows.len(), 2);
            assert_eq!(table.rows[0], vec!["Alice", "30", "NYC"]);
        } else {
            panic!("Expected table preview");
        }
    }

    #[test]
    fn test_json_array_as_table() {
        let json_data = br#"[{"name": "Alice", "age": 30}, {"name": "Bob", "age": 25}]"#;
        let result = parse_json(json_data).unwrap();

        if let PreviewData::Table(table) = result {
            assert_eq!(table.rows.len(), 2);
        } else {
            panic!("Expected table preview for JSON array of objects");
        }
    }

    #[test]
    fn test_json_object_as_pretty() {
        let json_data = br#"{"name": "Alice", "details": {"age": 30}}"#;
        let result = parse_json(json_data).unwrap();

        if let PreviewData::Json(json) = result {
            assert!(json.content.contains("Alice"));
            assert!(!json.is_raw); // Valid JSON should not be raw
        } else {
            panic!("Expected JSON preview for nested object");
        }
    }

    #[test]
    fn test_truncated_json_falls_back_to_raw() {
        // Simulate truncated JSON (incomplete array)
        let truncated_json = br#"[{"name": "Alice", "age": 30}, {"name": "Bob""#;
        let result = parse_json(truncated_json).unwrap();

        if let PreviewData::Json(json) = result {
            assert!(json.is_raw); // Should be raw since parsing failed
            assert!(json.truncated); // Should be marked as truncated
            assert!(json.content.contains("Alice")); // Content should still be there
        } else {
            panic!("Expected raw JSON preview for truncated JSON");
        }
    }

    #[test]
    fn test_truncated_utf8_handling() {
        // Create data with truncated multi-byte UTF-8 character at the end
        // '€' is 3 bytes: 0xE2 0x82 0xAC
        let mut data = br#"{"price": ""#.to_vec();
        data.push(0xE2); // First byte of '€'
        data.push(0x82); // Second byte of '€' - incomplete!

        let result = parse_json(&data).unwrap();

        if let PreviewData::Json(json) = result {
            assert!(json.is_raw); // Should be raw since it's incomplete
            assert!(json.content.contains("price")); // Should still have valid content
        } else {
            panic!("Expected raw JSON preview for truncated UTF-8");
        }
    }

    #[test]
    fn test_text_file_parsing() {
        let text_data = b"Hello, World!\nThis is a test file.\nLine 3.";
        let result = parse_text(text_data, "TXT").unwrap();

        if let PreviewData::Text(text) = result {
            assert_eq!(text.extension, "TXT");
            assert_eq!(text.total_lines, 3);
            assert!(text.content.contains("Hello, World!"));
            assert!(!text.truncated);
        } else {
            panic!("Expected text preview");
        }
    }

    #[test]
    fn test_binary_file_detection() {
        // Binary data with null bytes and non-UTF8 sequences
        let binary_data: &[u8] = &[0x00, 0x01, 0x02, 0xFF, 0xFE, 0x00, 0x89, 0x50, 0x4E, 0x47];

        let detected = PreviewFileType::detect_from_content(binary_data);
        assert_eq!(detected, PreviewFileType::Unsupported);
    }

    #[test]
    fn test_unknown_extension_with_text_content() {
        // Unknown extension but valid UTF-8 content
        let text_data = b"Some valid text content";
        let result = parse_preview(text_data, &PreviewFileType::Unsupported);

        // Should fall back to text detection and succeed
        assert!(result.is_ok());
        if let Ok(PreviewData::Text(text)) = result {
            assert_eq!(text.extension, "TEXT");
        } else {
            panic!("Expected text preview for unknown extension with valid UTF-8");
        }
    }

    #[test]
    fn test_unknown_extension_with_binary_content() {
        // Unknown extension with binary content
        let binary_data: &[u8] = &[0x00, 0x01, 0x02, 0xFF, 0xFE];
        let result = parse_preview(binary_data, &PreviewFileType::Unsupported);

        // Should fail with binary error
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("binary"));
    }
}
