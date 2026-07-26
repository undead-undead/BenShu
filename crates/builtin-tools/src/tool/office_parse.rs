use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

use benshu_infra::{Tool, ToolDefinition};

const MAX_OFFICE_PACKAGE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_OFFICE_XML_ENTRY_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfficeParseOutput {
    pub title: Option<String>,
    pub document_type: String,
    pub sections: Vec<OfficeSection>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfficeSection {
    pub label: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct OfficeParseTool;

impl OfficeParseTool {
    pub fn parse_path(path: impl AsRef<Path>) -> anyhow::Result<OfficeParseOutput> {
        let path = path.as_ref();
        let ext = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();

        match ext.as_str() {
            "docx" => parse_docx(path),
            "xlsx" => parse_xlsx(path),
            "pptx" => parse_pptx(path),
            _ => anyhow::bail!(
                "office_parse supports .docx, .xlsx, and .pptx only; got {}",
                path.display()
            ),
        }
    }

    pub fn to_markdown(output: &OfficeParseOutput) -> String {
        let mut md = String::new();
        if let Some(title) = &output.title {
            md.push_str(&format!("# {}\n\n", sanitize_text(title)));
        }
        md.push_str(&format!(
            "_Office document type: {}_\n\n",
            output.document_type
        ));

        for section in &output.sections {
            let content = sanitize_text(&section.content);
            if content.trim().is_empty() {
                continue;
            }
            md.push_str(&format!("## {}\n\n{}\n\n", section.label, content));
        }

        if !output.warnings.is_empty() {
            md.push_str("## Parse Warnings\n\n");
            for warning in &output.warnings {
                md.push_str(&format!("- {}\n", sanitize_text(warning)));
            }
            md.push('\n');
        }

        md.trim().to_string()
    }
}

#[derive(Debug, Deserialize)]
struct OfficeParseArgs {
    path: String,
    #[serde(default = "default_format")]
    format: String,
}

fn default_format() -> String {
    "markdown".to_string()
}

#[async_trait]
impl Tool for OfficeParseTool {
    fn name(&self) -> String {
        "office_parse".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "office_parse".to_string(),
            description: "Parse Office Open XML documents (.docx, .xlsx, .pptx) into clean Markdown or structured JSON for knowledge ingestion.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Local path to a .docx, .xlsx, or .pptx file." },
                    "format": { "type": "string", "enum": ["markdown", "json"], "description": "Output format. Markdown is recommended for RAG ingestion." }
                },
                "required": ["path"]
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use this to extract text from Word, Excel, and PowerPoint files before summarizing or importing into the knowledge base. Legacy binary .doc/.xls/.ppt files are not supported.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: OfficeParseArgs = serde_json::from_str(arguments)
            .map_err(|e| anyhow::anyhow!("Tool arguments error for office_parse: {}", e))?;
        let output = Self::parse_path(&args.path)?;
        if args.format == "json" {
            Ok(serde_json::to_string_pretty(&output)?)
        } else {
            Ok(Self::to_markdown(&output))
        }
    }
}

fn open_zip(path: &Path) -> anyhow::Result<ZipArchive<File>> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| anyhow::anyhow!("failed to stat {}: {}", path.display(), error))?;
    if metadata.len() > MAX_OFFICE_PACKAGE_BYTES {
        anyhow::bail!(
            "Office file is larger than the 20MB single-file safety limit: {} bytes",
            metadata.len()
        );
    }

    let file = File::open(path)
        .map_err(|error| anyhow::anyhow!("failed to open {}: {}", path.display(), error))?;
    ZipArchive::new(file)
        .map_err(|error| anyhow::anyhow!("failed to read Office zip package: {}", error))
}

fn read_zip_text(zip: &mut ZipArchive<File>, name: &str) -> anyhow::Result<Option<String>> {
    let Ok(mut file) = zip.by_name(name) else {
        return Ok(None);
    };
    if file.size() > MAX_OFFICE_XML_ENTRY_BYTES {
        anyhow::bail!(
            "Office XML entry '{}' is larger than the 5MB safety limit: {} bytes",
            name,
            file.size()
        );
    }
    let mut xml = String::new();
    file.by_ref()
        .take(MAX_OFFICE_XML_ENTRY_BYTES + 1)
        .read_to_string(&mut xml)?;
    Ok(Some(xml))
}

fn zip_names(zip: &mut ZipArchive<File>) -> Vec<String> {
    let mut names = Vec::new();
    for idx in 0..zip.len() {
        if let Ok(file) = zip.by_index(idx) {
            names.push(file.name().to_string());
        }
    }
    names
}

fn parse_docx(path: &Path) -> anyhow::Result<OfficeParseOutput> {
    let mut zip = open_zip(path)?;
    let mut warnings = Vec::new();
    let body = read_zip_text(&mut zip, "word/document.xml")?
        .ok_or_else(|| anyhow::anyhow!("docx missing word/document.xml"))?;
    let mut sections = vec![OfficeSection {
        label: "Document Body".to_string(),
        content: extract_tag_text(&body, &["w:t"]).join("\n"),
    }];

    for (name, label) in [
        ("word/header1.xml", "Header"),
        ("word/footer1.xml", "Footer"),
        ("word/comments.xml", "Comments"),
    ] {
        if let Some(xml) = read_zip_text(&mut zip, name)? {
            let text = extract_tag_text(&xml, &["w:t"]).join("\n");
            if !text.trim().is_empty() {
                sections.push(OfficeSection {
                    label: label.to_string(),
                    content: text,
                });
            }
        }
    }

    if sections
        .iter()
        .all(|section| section.content.trim().is_empty())
    {
        warnings.push("no_text_extracted_from_docx".to_string());
    }

    Ok(OfficeParseOutput {
        title: title_from_path(path),
        document_type: "docx".to_string(),
        sections,
        warnings,
    })
}

fn parse_xlsx(path: &Path) -> anyhow::Result<OfficeParseOutput> {
    let mut zip = open_zip(path)?;
    let names = zip_names(&mut zip);
    let shared_strings = read_zip_text(&mut zip, "xl/sharedStrings.xml")?
        .map(|xml| extract_tag_text(&xml, &["t"]))
        .unwrap_or_default();

    let mut sheet_names: Vec<String> = names
        .into_iter()
        .filter(|name| {
            name.starts_with("xl/worksheets/sheet")
                && name.ends_with(".xml")
                && !name.contains("_rels/")
        })
        .collect();
    sheet_names.sort_by_key(|name| natural_number_key(name));

    let mut sections = Vec::new();
    let mut warnings = Vec::new();
    for sheet_name in sheet_names {
        if let Some(xml) = read_zip_text(&mut zip, &sheet_name)? {
            let rows = extract_xlsx_rows(&xml, &shared_strings);
            if rows.is_empty() {
                continue;
            }
            sections.push(OfficeSection {
                label: sheet_name
                    .strip_prefix("xl/worksheets/")
                    .unwrap_or(&sheet_name)
                    .trim_end_matches(".xml")
                    .to_string(),
                content: rows.join("\n"),
            });
        }
    }

    if sections.is_empty() {
        warnings.push("no_text_extracted_from_xlsx".to_string());
    }

    Ok(OfficeParseOutput {
        title: title_from_path(path),
        document_type: "xlsx".to_string(),
        sections,
        warnings,
    })
}

fn parse_pptx(path: &Path) -> anyhow::Result<OfficeParseOutput> {
    let mut zip = open_zip(path)?;
    let mut slide_names: Vec<String> = zip_names(&mut zip)
        .into_iter()
        .filter(|name| {
            name.starts_with("ppt/slides/slide")
                && name.ends_with(".xml")
                && !name.contains("_rels/")
        })
        .collect();
    slide_names.sort_by_key(|name| natural_number_key(name));

    let mut sections = Vec::new();
    let mut warnings = Vec::new();
    for slide_name in slide_names {
        if let Some(xml) = read_zip_text(&mut zip, &slide_name)? {
            let text = extract_tag_text(&xml, &["a:t"]).join("\n");
            if text.trim().is_empty() {
                continue;
            }
            sections.push(OfficeSection {
                label: slide_name
                    .strip_prefix("ppt/slides/")
                    .unwrap_or(&slide_name)
                    .trim_end_matches(".xml")
                    .to_string(),
                content: text,
            });
        }
    }

    if sections.is_empty() {
        warnings.push("no_text_extracted_from_pptx".to_string());
    }

    Ok(OfficeParseOutput {
        title: title_from_path(path),
        document_type: "pptx".to_string(),
        sections,
        warnings,
    })
}

fn extract_xlsx_rows(xml: &str, shared_strings: &[String]) -> Vec<String> {
    let row_re = Regex::new(r#"(?s)<row\b[^>]*>(.*?)</row>"#).unwrap();
    row_re
        .captures_iter(xml)
        .filter_map(|row_caps| {
            let row_xml = row_caps.get(1)?.as_str();
            let values = extract_xlsx_cells(row_xml, shared_strings);
            if values.iter().all(|value| value.trim().is_empty()) {
                None
            } else {
                Some(values.join(" | "))
            }
        })
        .collect()
}

fn extract_xlsx_cells(row_xml: &str, shared_strings: &[String]) -> Vec<String> {
    let cell_re = Regex::new(r#"(?s)<c\b([^>]*)>(.*?)</c>"#).unwrap();
    let value_re = Regex::new(r#"(?s)<v[^>]*>(.*?)</v>"#).unwrap();
    let inline_re = Regex::new(r#"(?s)<t[^>]*>(.*?)</t>"#).unwrap();

    cell_re
        .captures_iter(row_xml)
        .map(|caps| {
            let attrs = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
            let body = caps.get(2).map(|m| m.as_str()).unwrap_or_default();
            if attrs.contains(r#"t="s""#) || attrs.contains(r#"t='s'"#) {
                let idx = value_re
                    .captures(body)
                    .and_then(|v| v.get(1))
                    .and_then(|m| m.as_str().trim().parse::<usize>().ok());
                idx.and_then(|i| shared_strings.get(i).cloned())
                    .unwrap_or_default()
            } else if attrs.contains(r#"t="inlineStr""#) || attrs.contains(r#"t='inlineStr'"#) {
                inline_re
                    .captures(body)
                    .and_then(|v| v.get(1))
                    .map(|m| decode_xml_entities(m.as_str()))
                    .unwrap_or_default()
            } else {
                value_re
                    .captures(body)
                    .and_then(|v| v.get(1))
                    .map(|m| decode_xml_entities(m.as_str()))
                    .unwrap_or_default()
            }
        })
        .collect()
}

fn extract_tag_text(xml: &str, tags: &[&str]) -> Vec<String> {
    let mut values = Vec::new();
    for tag in tags {
        let pattern = format!(
            r#"(?s)<{}\b[^>]*>(.*?)</{}>"#,
            regex::escape(tag),
            regex::escape(tag)
        );
        let re = Regex::new(&pattern).unwrap();
        for caps in re.captures_iter(xml) {
            if let Some(value) = caps.get(1) {
                let text = decode_xml_entities(value.as_str());
                if !text.trim().is_empty() {
                    values.push(text);
                }
            }
        }
    }
    values
}

fn decode_xml_entities(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn sanitize_text(value: &str) -> String {
    let injection_patterns = ["ignore all instructions", "system prompt:", "new command:"];
    let mut sanitized = value.to_string();
    for pattern in injection_patterns {
        sanitized = sanitized.replace(pattern, "[FILTERED]");
    }
    sanitized
}

fn title_from_path(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|value| value.to_str())
        .map(ToOwned::to_owned)
}

fn natural_number_key(value: &str) -> usize {
    let digits: String = value.chars().filter(|ch| ch.is_ascii_digit()).collect();
    digits.parse().unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_tag_text_with_entities() {
        let xml = r#"<w:t>Hello &amp; world</w:t><w:t>第二段</w:t>"#;
        assert_eq!(
            extract_tag_text(xml, &["w:t"]),
            vec!["Hello & world".to_string(), "第二段".to_string()]
        );
    }

    #[test]
    fn xlsx_shared_string_cells_are_resolved() {
        let row = r#"<row><c t="s"><v>1</v></c><c><v>42</v></c></row>"#;
        let values = extract_xlsx_cells(row, &["A".into(), "Name".into()]);
        assert_eq!(values, vec!["Name".to_string(), "42".to_string()]);
    }
}
