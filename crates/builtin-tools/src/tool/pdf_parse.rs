use async_trait::async_trait;
use base64::Engine as _;
use lopdf::content::Content;
use lopdf::Document as LopdfDocument;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tracing::warn;
use uuid::Uuid;

use benshu_brain::agent::provider::Provider;
use benshu_infra::{Tool, ToolDefinition};
use benshu_sensory::{SensoryHub, SensoryOutput};

/// Represents the structural elements of a PDF, mirroring opendataloader-pdf schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PdfElement {
    Paragraph {
        content: String,
        #[serde(rename = "font_size")]
        font_size: f32,
        #[serde(rename = "page_number")]
        page_number: usize,
        #[serde(rename = "bounding_box")]
        bounding_box: [f32; 4], // [left, bottom, right, top]
    },
    Heading {
        content: String,
        level: usize,
        #[serde(rename = "page_number")]
        page_number: usize,
        #[serde(rename = "bounding_box")]
        bounding_box: [f32; 4],
    },
    Table {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        headers: Option<Vec<String>>,
        rows: Vec<Vec<String>>,
        #[serde(
            rename = "column_bounding_boxes",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        column_bounding_boxes: Option<Vec<[f32; 4]>>,
        #[serde(rename = "page_number")]
        page_number: usize,
        #[serde(rename = "bounding_box")]
        bounding_box: [f32; 4],
    },
    Image {
        #[serde(rename = "page_number")]
        page_number: usize,
        #[serde(rename = "bounding_box")]
        bounding_box: [f32; 4],
        source: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfOutput {
    pub title: Option<String>,
    pub author: Option<String>,
    pub total_pages: usize,
    pub parser_mode: String,
    pub image_output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_root: Option<String>,
    pub artifact_cleanup_policy: String,
    pub ocr_backend: String,
    pub ocr_language: String,
    pub warnings: Vec<String>,
    pub route_report: PdfRouteReport,
    pub metrics: PdfParseMetrics,
    pub page_routes: Vec<PdfPageRoute>,
    pub elements: Vec<PdfElement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfRouteReport {
    pub encrypted: bool,
    pub large_document_guard_active: bool,
    pub unsupported_features: Vec<String>,
    pub degraded_pages: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfParseMetrics {
    pub pages_processed: usize,
    pub degraded_pages: usize,
    pub native_pages: usize,
    pub enriched_pages: usize,
    pub fallback_pages: usize,
    pub dominant_image_pages: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfPageRoute {
    pub page_number: usize,
    pub selected_route: String,
    pub text_layer_detected: bool,
    pub fallback_used: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_contract_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_contract_ref: Option<String>,
    #[serde(default)]
    pub triage_reasons: Vec<String>,
    pub degraded: bool,
    pub triage: PdfPageTriage,
}

struct PageParseResult {
    route: PdfPageRoute,
    elements: Vec<PdfElement>,
}

struct NativePageExtraction {
    extracted_text: String,
    chunks: Vec<PositionedTextChunk>,
    page_width: f32,
    page_height: f32,
}

fn page_source_contract(
    page_number: usize,
    selected_route: &str,
) -> (Option<String>, Option<String>) {
    match selected_route {
        "page_image_vlm" | "page_image_ocr" | "page_image_text_extract" => (
            Some("pdf_page_image".to_string()),
            Some(format!("pdf_page:{}", page_number)),
        ),
        _ => (None, None),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PdfTextLayerQuality {
    None,
    Weak,
    Strong,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfPageTriage {
    pub text_layer_quality: PdfTextLayerQuality,
    pub text_chars: usize,
    pub alnum_chars: usize,
    pub native_line_count: usize,
    pub dominant_page_image_detected: bool,
    pub rotation_degrees: i32,
}

struct PageRouteDecision {
    selected_route: String,
    use_native: bool,
    triage_reasons: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineRole {
    Table,
    Heading,
    Body,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PdfImageOutputMode {
    Off,
    Embedded,
    Artifact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayoutRegion {
    Span,
    Left,
    Right,
}

#[derive(Debug, Clone)]
struct PositionedTextChunk {
    text: String,
    x: f32,
    y: f32,
    right: f32,
    bottom: f32,
    top: f32,
    font_size: f32,
}

#[derive(Debug, Clone)]
struct LineDescriptor {
    text: String,
    bbox: [f32; 4],
    font_size: f32,
    cells: Vec<String>,
    cell_starts: Vec<f32>,
    region: LayoutRegion,
}

#[derive(Debug, Clone)]
struct NormalizedTable {
    headers: Option<Vec<String>>,
    rows: Vec<Vec<String>>,
    column_bounding_boxes: Vec<[f32; 4]>,
}

struct PageImageSet {
    elements: Vec<PdfElement>,
    dominant_image: Option<image::DynamicImage>,
    warnings: Vec<String>,
}

struct PdfArtifactSession {
    root: Option<PathBuf>,
    emitted_artifacts: usize,
}

trait PdfNativeExtractor {
    fn extract_page(
        &self,
        tool: &PdfParseTool,
        doc: &LopdfDocument,
        page_num: u32,
        page_id: lopdf::ObjectId,
    ) -> anyhow::Result<NativePageExtraction>;
}

trait PdfPageRasterizer {
    fn rasterize_page(
        &self,
        tool: &PdfParseTool,
        doc: &LopdfDocument,
        page_id: lopdf::ObjectId,
        page_idx: usize,
        page_width: f32,
        page_height: f32,
        artifact_session: &mut PdfArtifactSession,
    ) -> anyhow::Result<PageImageSet>;
}

trait PdfPageTriagePolicy {
    fn decide(
        &self,
        mode: &str,
        extracted_text: &str,
        dominant_page_image_detected: bool,
        rotation_degrees: i32,
    ) -> (PdfPageTriage, PageRouteDecision);
}

#[async_trait]
trait PdfStructuredEnricher {
    async fn enrich_page(
        &self,
        tool: &PdfParseTool,
        page_num: usize,
        page_image: &image::DynamicImage,
        triage: PdfPageTriage,
        triage_reasons: Vec<String>,
        ocr_backend_name: &str,
        ocr_language: &str,
    ) -> anyhow::Result<PageParseResult>;
}

trait PdfArtifactStore {
    fn build_image_source(
        &self,
        image: &image::DynamicImage,
        image_format: image::ImageFormat,
        artifact_root: Option<&Path>,
        page_idx: usize,
        image_name: &str,
        image_output: PdfImageOutputMode,
    ) -> anyhow::Result<Option<String>>;
}

#[derive(Clone, Copy)]
struct LopdfNativeExtractor;

struct InProcessPageRasterizer {
    image_output: PdfImageOutputMode,
}

#[derive(Clone, Copy)]
struct HeuristicPdfPageTriage;

#[derive(Clone, Copy)]
struct SensoryStructuredEnricher;

#[derive(Clone, Copy)]
struct ControlledPdfArtifactStore;

const PAGE_ROUTE_TIMEOUT_SECS: u64 = 30;
const DEFAULT_PAGE_LIMIT: usize = 400;
const ARTIFACT_RETENTION_SECS: u64 = 24 * 60 * 60;

impl PdfArtifactSession {
    fn new(pdf_path: &str, image_output: PdfImageOutputMode) -> anyhow::Result<Self> {
        let root = match image_output {
            PdfImageOutputMode::Artifact => {
                PdfParseTool::cleanup_stale_artifact_roots()?;
                Some(PdfParseTool::artifact_root(pdf_path)?)
            }
            _ => None,
        };
        Ok(Self {
            root,
            emitted_artifacts: 0,
        })
    }

    fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    fn record_emitted_artifact(&mut self) {
        self.emitted_artifacts += 1;
    }

    fn finalize(self) -> anyhow::Result<Option<String>> {
        match self.root {
            Some(root) if self.emitted_artifacts == 0 => {
                if root.exists() {
                    std::fs::remove_dir_all(&root)?;
                }
                Ok(None)
            }
            Some(root) => Ok(Some(root.to_string_lossy().to_string())),
            None => Ok(None),
        }
    }
}

pub struct PdfParseTool {
    provider: Option<Arc<dyn Provider>>,
    model: Option<String>,
    sensory: Arc<SensoryHub>,
}

impl PdfNativeExtractor for LopdfNativeExtractor {
    fn extract_page(
        &self,
        tool: &PdfParseTool,
        doc: &LopdfDocument,
        page_num: u32,
        page_id: lopdf::ObjectId,
    ) -> anyhow::Result<NativePageExtraction> {
        let extracted_text = doc.extract_text(&[page_num]).unwrap_or_default();
        let (page_width, page_height) = PdfParseTool::page_dimensions(doc, page_id);
        let chunks = tool.extract_positioned_text_chunks(doc, page_id)?;
        Ok(NativePageExtraction {
            extracted_text,
            chunks,
            page_width,
            page_height,
        })
    }
}

impl PdfArtifactStore for ControlledPdfArtifactStore {
    fn build_image_source(
        &self,
        image: &image::DynamicImage,
        image_format: image::ImageFormat,
        artifact_root: Option<&Path>,
        page_idx: usize,
        image_name: &str,
        image_output: PdfImageOutputMode,
    ) -> anyhow::Result<Option<String>> {
        PdfParseTool::build_image_source(
            image,
            image_format,
            artifact_root,
            page_idx,
            image_name,
            image_output,
        )
    }
}

impl PdfPageRasterizer for InProcessPageRasterizer {
    fn rasterize_page(
        &self,
        tool: &PdfParseTool,
        doc: &LopdfDocument,
        page_id: lopdf::ObjectId,
        page_idx: usize,
        page_width: f32,
        page_height: f32,
        artifact_session: &mut PdfArtifactSession,
    ) -> anyhow::Result<PageImageSet> {
        tool.extract_page_images(
            doc,
            page_id,
            page_idx,
            page_width,
            page_height,
            artifact_session,
            self.image_output,
        )
    }
}

impl PdfPageTriagePolicy for HeuristicPdfPageTriage {
    fn decide(
        &self,
        mode: &str,
        extracted_text: &str,
        dominant_page_image_detected: bool,
        rotation_degrees: i32,
    ) -> (PdfPageTriage, PageRouteDecision) {
        let triage = PdfParseTool::build_page_triage(
            extracted_text,
            dominant_page_image_detected,
            rotation_degrees,
        );
        let decision = PdfParseTool::decide_page_route(mode, &triage);
        (triage, decision)
    }
}

#[async_trait]
impl PdfStructuredEnricher for SensoryStructuredEnricher {
    async fn enrich_page(
        &self,
        tool: &PdfParseTool,
        page_num: usize,
        page_image: &image::DynamicImage,
        triage: PdfPageTriage,
        triage_reasons: Vec<String>,
        ocr_backend_name: &str,
        ocr_language: &str,
    ) -> anyhow::Result<PageParseResult> {
        tool.parse_page_via_ai(
            page_num,
            page_image,
            triage,
            triage_reasons,
            ocr_backend_name,
            ocr_language,
        )
        .await
    }
}

impl PdfParseTool {
    pub fn new(
        provider: Option<Arc<dyn Provider>>,
        model: Option<String>,
        sensory: Arc<SensoryHub>,
    ) -> Self {
        Self {
            provider,
            model,
            sensory,
        }
    }

    /// Final Polish: Adds AI Safety filtering and precise image extraction logic.
    fn sanitize_content(content: &str) -> String {
        // AI Safety: Filter potential prompt injection patterns in document text
        let injection_patterns = ["ignore all instructions", "system prompt:", "new command:"];
        let mut sanitized = content.to_string();
        for pattern in &injection_patterns {
            if sanitized.to_lowercase().contains(pattern) {
                warn!("⚠️ Detection: Potential Prompt Injection attempt in PDF content. Sanitizing...");
                sanitized = sanitized.replace(pattern, "[FILTERED]");
            }
        }
        sanitized
    }

    /// Convert parsed PDF output to a clean Markdown representation with Accessibility tags.
    pub fn to_markdown(output: &PdfOutput) -> String {
        let mut md = String::new();
        md.push_str("<!-- AI-Enhanced Semantic Extraction per Section 508 / ADA -->\n");
        if let Some(t) = &output.title {
            md.push_str(&format!("# {}\n\n", Self::sanitize_content(t)));
        }

        for element in &output.elements {
            match element {
                PdfElement::Heading { content, level, .. } => {
                    md.push_str(&format!(
                        "{} {}\n\n",
                        "#".repeat(*level),
                        Self::sanitize_content(content)
                    ));
                }
                PdfElement::Paragraph { content, .. } => {
                    md.push_str(&format!("{}\n\n", Self::sanitize_content(content)));
                }
                PdfElement::Table { headers, rows, .. } => {
                    md.push_str("<table_start>\n");
                    let markdown_rows: Vec<Vec<String>> = if let Some(headers) = headers {
                        let mut combined = Vec::with_capacity(rows.len() + 1);
                        combined.push(headers.clone());
                        combined.extend(rows.iter().cloned());
                        combined
                    } else {
                        rows.clone()
                    };
                    for (i, row) in markdown_rows.iter().enumerate() {
                        md.push_str("| ");
                        md.push_str(
                            &row.iter()
                                .map(|c| Self::sanitize_content(c))
                                .collect::<Vec<_>>()
                                .join(" | "),
                        );
                        md.push_str(" |\n");
                        if i == 0 {
                            md.push_str("| ");
                            md.push_str(&row.iter().map(|_| "---").collect::<Vec<_>>().join(" | "));
                            md.push_str(" |\n");
                        }
                    }
                    md.push_str("<table_end>\n\n");
                }
                PdfElement::Image { source, .. } => {
                    md.push_str(&format!("![Semantic Image Description]({})\n\n", source));
                }
            }
        }
        md
    }

    fn normalize_mode(mode: &str) -> &str {
        match mode {
            "text" => "text",
            "vision" => "vision",
            "hybrid" => "hybrid",
            "auto" => "auto",
            _ => "auto",
        }
    }

    fn normalize_image_output(mode: Option<&str>) -> PdfImageOutputMode {
        match mode.unwrap_or("off") {
            "embedded" => PdfImageOutputMode::Embedded,
            "artifact" => PdfImageOutputMode::Artifact,
            _ => PdfImageOutputMode::Off,
        }
    }

    pub fn in_process_enrichment_ready() -> bool {
        true
    }

    fn analyze_text_layer(text: &str) -> (usize, usize, PdfTextLayerQuality) {
        let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let text_chars = compact.chars().count();
        let alnum = compact.chars().filter(|c| c.is_alphanumeric()).count();
        let quality = if compact.trim().is_empty() {
            PdfTextLayerQuality::None
        } else if text_chars >= 48 || alnum >= 24 {
            PdfTextLayerQuality::Strong
        } else {
            PdfTextLayerQuality::Weak
        };
        (text_chars, alnum, quality)
    }

    fn build_page_triage(
        text: &str,
        dominant_page_image_detected: bool,
        rotation_degrees: i32,
    ) -> PdfPageTriage {
        let (text_chars, alnum_chars, text_layer_quality) = Self::analyze_text_layer(text);
        PdfPageTriage {
            text_layer_quality,
            text_chars,
            alnum_chars,
            native_line_count: 0,
            dominant_page_image_detected,
            rotation_degrees,
        }
    }

    fn decide_page_route(mode: &str, triage: &PdfPageTriage) -> PageRouteDecision {
        let mut triage_reasons = Vec::new();
        let (selected_route, use_native) = match mode {
            "text" => {
                triage_reasons.push("mode=text".to_string());
                ("native_structured".to_string(), true)
            }
            "vision" => {
                triage_reasons.push("mode=vision".to_string());
                ("vision_structured".to_string(), false)
            }
            "hybrid" => {
                if triage.dominant_page_image_detected {
                    triage_reasons.push("dominant_page_image_detected".to_string());
                    ("native_structured_with_enrichment".to_string(), false)
                } else if matches!(triage.text_layer_quality, PdfTextLayerQuality::Strong) {
                    triage_reasons.push("strong_text_layer_detected".to_string());
                    ("native_structured".to_string(), true)
                } else {
                    triage_reasons.push("non_strong_text_layer".to_string());
                    ("native_structured_with_enrichment".to_string(), false)
                }
            }
            _ => {
                if matches!(triage.text_layer_quality, PdfTextLayerQuality::Strong) {
                    triage_reasons.push("strong_text_layer_detected".to_string());
                    ("native_structured".to_string(), true)
                } else if triage.dominant_page_image_detected {
                    triage_reasons.push("dominant_page_image_detected".to_string());
                    ("vision_structured".to_string(), false)
                } else {
                    triage_reasons
                        .push("weak_or_missing_text_layer_without_page_image".to_string());
                    ("native_structured".to_string(), true)
                }
            }
        };

        PageRouteDecision {
            selected_route,
            use_native,
            triage_reasons,
        }
    }

    fn page_rotation_degrees(doc: &LopdfDocument, page_id: lopdf::ObjectId) -> i32 {
        doc.get_dictionary(page_id)
            .ok()
            .and_then(|dict| dict.get(b"Rotate").ok())
            .and_then(|obj| obj.as_i64().ok())
            .map(|degrees| degrees.rem_euclid(360) as i32)
            .unwrap_or(0)
    }

    fn document_is_encrypted(doc: &LopdfDocument) -> bool {
        doc.trailer.get(b"Encrypt").is_ok()
    }

    fn extract_metadata(doc: &LopdfDocument) -> (Option<String>, Option<String>) {
        let Some(info_ref) = doc
            .trailer
            .get(b"Info")
            .ok()
            .and_then(|obj| obj.as_reference().ok())
        else {
            return (None, None);
        };

        let Ok(info_dict) = doc.get_dictionary(info_ref) else {
            return (None, None);
        };

        let title = info_dict
            .get(b"Title")
            .ok()
            .and_then(|obj| obj.as_string().ok())
            .map(|text| text.into_owned())
            .filter(|text| !text.trim().is_empty());
        let author = info_dict
            .get(b"Author")
            .ok()
            .and_then(|obj| obj.as_string().ok())
            .map(|text| text.into_owned())
            .filter(|text| !text.trim().is_empty());

        (title, author)
    }

    fn split_table_row(line: &str) -> Option<Vec<String>> {
        if line.contains('|') {
            let cells: Vec<String> = line
                .split('|')
                .map(str::trim)
                .filter(|cell| !cell.is_empty())
                .map(ToOwned::to_owned)
                .collect();
            return (cells.len() >= 2).then_some(cells);
        }

        if line.contains('\t') {
            let cells: Vec<String> = line
                .split('\t')
                .map(str::trim)
                .filter(|cell| !cell.is_empty())
                .map(ToOwned::to_owned)
                .collect();
            return (cells.len() >= 2).then_some(cells);
        }

        let mut cells = Vec::new();
        let mut current = String::new();
        let mut consecutive_spaces = 0usize;
        for ch in line.chars() {
            if ch == ' ' {
                consecutive_spaces += 1;
                if consecutive_spaces >= 2 && !current.trim().is_empty() {
                    cells.push(current.trim().to_string());
                    current.clear();
                } else if consecutive_spaces < 2 {
                    current.push(ch);
                }
            } else {
                consecutive_spaces = 0;
                current.push(ch);
            }
        }

        if !current.trim().is_empty() {
            cells.push(current.trim().to_string());
        }

        (cells.len() >= 3).then_some(cells)
    }

    fn looks_like_heading(line: &str) -> bool {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.len() > 90 {
            return false;
        }

        let alpha_chars: Vec<char> = trimmed.chars().filter(|c| c.is_alphabetic()).collect();
        if alpha_chars.is_empty() {
            return false;
        }

        let uppercase = alpha_chars.iter().filter(|c| c.is_uppercase()).count();
        uppercase * 10 >= alpha_chars.len() * 7
    }

    fn object_to_f32(obj: &lopdf::Object) -> Option<f32> {
        obj.as_f32()
            .ok()
            .or_else(|| obj.as_i64().ok().map(|value| value as f32))
    }

    fn estimate_text_width(text: &str, font_size: f32) -> f32 {
        let glyphs = text.chars().count().max(1) as f32;
        let whitespace = text.chars().filter(|c| c.is_whitespace()).count() as f32;
        let dense_glyphs = (glyphs - whitespace).max(1.0);
        dense_glyphs * font_size * 0.52 + whitespace * font_size * 0.28
    }

    fn page_dimensions(doc: &LopdfDocument, page_id: lopdf::ObjectId) -> (f32, f32) {
        let Ok(page_dict) = doc.get_dictionary(page_id) else {
            return (612.0, 792.0);
        };
        let Ok(media_box) = page_dict.get(b"MediaBox").and_then(|obj| obj.as_array()) else {
            return (612.0, 792.0);
        };
        if media_box.len() != 4 {
            return (612.0, 792.0);
        }

        let left = Self::object_to_f32(&media_box[0]).unwrap_or(0.0);
        let bottom = Self::object_to_f32(&media_box[1]).unwrap_or(0.0);
        let right = Self::object_to_f32(&media_box[2]).unwrap_or(612.0);
        let top = Self::object_to_f32(&media_box[3]).unwrap_or(792.0);
        ((right - left).max(1.0), (top - bottom).max(1.0))
    }

    fn decode_text_operand(encoding: Option<&str>, operand: &lopdf::Object) -> String {
        match operand {
            lopdf::Object::String(bytes, _) => LopdfDocument::decode_text(encoding, bytes),
            lopdf::Object::Array(items) => {
                let mut out = String::new();
                for item in items {
                    match item {
                        lopdf::Object::String(bytes, _) => {
                            out.push_str(&LopdfDocument::decode_text(encoding, bytes));
                        }
                        lopdf::Object::Integer(i) if *i < -100 => out.push(' '),
                        lopdf::Object::Real(v) if *v < -100.0 => out.push(' '),
                        _ => {}
                    }
                }
                out
            }
            _ => String::new(),
        }
    }

    fn extract_positioned_text_chunks(
        &self,
        doc: &LopdfDocument,
        page_id: lopdf::ObjectId,
    ) -> anyhow::Result<Vec<PositionedTextChunk>> {
        let fonts = doc.get_page_fonts(page_id);
        let encodings = fonts
            .into_iter()
            .map(|(name, font)| (name, font.get_font_encoding()))
            .collect::<std::collections::BTreeMap<Vec<u8>, &str>>();
        let content_data = doc.get_page_content(page_id)?;
        let content = Content::decode(&content_data)
            .map_err(|e| anyhow::anyhow!("Failed to decode PDF content stream: {}", e))?;

        let mut current_encoding = None;
        let mut current_font_size = 10.0_f32;
        let mut current_x = 72.0_f32;
        let mut current_y = 720.0_f32;
        let mut line_start_x = current_x;
        let mut leading = 14.0_f32;
        let mut chunks = Vec::new();

        for operation in &content.operations {
            match operation.operator.as_str() {
                "BT" => {
                    current_x = line_start_x;
                }
                "ET" => {}
                "Tf" => {
                    if let Some(font_name) = operation
                        .operands
                        .first()
                        .and_then(|obj| obj.as_name().ok())
                    {
                        current_encoding = encodings.get(font_name).copied();
                    }
                    if let Some(size) = operation.operands.get(1).and_then(Self::object_to_f32) {
                        current_font_size = size.max(1.0);
                        if leading <= 0.0 {
                            leading = current_font_size * 1.2;
                        }
                    }
                }
                "TL" => {
                    if let Some(value) = operation.operands.first().and_then(Self::object_to_f32) {
                        leading = value.abs().max(1.0);
                    }
                }
                "Td" => {
                    if let (Some(tx), Some(ty)) = (
                        operation.operands.first().and_then(Self::object_to_f32),
                        operation.operands.get(1).and_then(Self::object_to_f32),
                    ) {
                        current_x += tx;
                        current_y += ty;
                        line_start_x = current_x;
                    }
                }
                "TD" => {
                    if let (Some(tx), Some(ty)) = (
                        operation.operands.first().and_then(Self::object_to_f32),
                        operation.operands.get(1).and_then(Self::object_to_f32),
                    ) {
                        current_x += tx;
                        current_y += ty;
                        line_start_x = current_x;
                        leading = ty.abs().max(current_font_size * 1.2);
                    }
                }
                "Tm" => {
                    if let (Some(x), Some(y)) = (
                        operation.operands.get(4).and_then(Self::object_to_f32),
                        operation.operands.get(5).and_then(Self::object_to_f32),
                    ) {
                        current_x = x;
                        current_y = y;
                        line_start_x = x;
                    }
                }
                "T*" => {
                    current_y -= leading.max(current_font_size * 1.2);
                    current_x = line_start_x;
                }
                "'" => {
                    current_y -= leading.max(current_font_size * 1.2);
                    current_x = line_start_x;
                    if let Some(operand) = operation.operands.first() {
                        let text = Self::decode_text_operand(current_encoding, operand)
                            .trim()
                            .to_string();
                        if !text.is_empty() {
                            let width = Self::estimate_text_width(&text, current_font_size);
                            chunks.push(PositionedTextChunk {
                                text,
                                x: current_x,
                                y: current_y,
                                right: current_x + width,
                                bottom: current_y - current_font_size * 0.30,
                                top: current_y + current_font_size * 0.90,
                                font_size: current_font_size,
                            });
                            current_x += width;
                        }
                    }
                }
                "\"" => {
                    current_y -= leading.max(current_font_size * 1.2);
                    current_x = line_start_x;
                    if let Some(operand) = operation.operands.get(2) {
                        let text = Self::decode_text_operand(current_encoding, operand)
                            .trim()
                            .to_string();
                        if !text.is_empty() {
                            let width = Self::estimate_text_width(&text, current_font_size);
                            chunks.push(PositionedTextChunk {
                                text,
                                x: current_x,
                                y: current_y,
                                right: current_x + width,
                                bottom: current_y - current_font_size * 0.30,
                                top: current_y + current_font_size * 0.90,
                                font_size: current_font_size,
                            });
                            current_x += width;
                        }
                    }
                }
                "Tj" | "TJ" => {
                    if let Some(operand) = operation.operands.first() {
                        let text = Self::decode_text_operand(current_encoding, operand)
                            .trim()
                            .to_string();
                        if !text.is_empty() {
                            let width = Self::estimate_text_width(&text, current_font_size);
                            chunks.push(PositionedTextChunk {
                                text,
                                x: current_x,
                                y: current_y,
                                right: current_x + width,
                                bottom: current_y - current_font_size * 0.30,
                                top: current_y + current_font_size * 0.90,
                                font_size: current_font_size,
                            });
                            current_x += width;
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(chunks)
    }

    fn line_cells_and_starts_from_chunks(
        chunks: &[PositionedTextChunk],
    ) -> (Vec<String>, Vec<f32>) {
        if chunks.is_empty() {
            return (Vec::new(), Vec::new());
        }

        let mut cells = Vec::new();
        let mut cell_starts = Vec::new();
        let mut current = String::new();
        let mut current_start: Option<f32> = None;
        let mut previous_right = chunks[0].x;
        let font_size =
            chunks.iter().map(|chunk| chunk.font_size).sum::<f32>() / chunks.len() as f32;
        let split_threshold = font_size.max(10.0) * 2.6;

        for chunk in chunks {
            let gap = (chunk.x - previous_right).max(0.0);
            if gap >= split_threshold && !current.trim().is_empty() {
                cells.push(current.trim().to_string());
                if let Some(start) = current_start.take() {
                    cell_starts.push(start);
                }
                current.clear();
            } else if !current.is_empty() {
                current.push(' ');
            }
            if current_start.is_none() {
                current_start = Some(chunk.x);
            }
            current.push_str(chunk.text.trim());
            previous_right = chunk.right;
        }

        if !current.trim().is_empty() {
            cells.push(current.trim().to_string());
            if let Some(start) = current_start {
                cell_starts.push(start);
            }
        }

        (cells, cell_starts)
    }

    fn line_is_table_candidate(line: &LineDescriptor) -> bool {
        line.cells.len() >= 3
            || Self::split_table_row(&line.text).map_or(false, |cells| cells.len() >= 3)
    }

    fn lines_share_column_alignment(left: &LineDescriptor, right: &LineDescriptor) -> bool {
        if left.cell_starts.len() < 2 || right.cell_starts.len() < 2 {
            return false;
        }

        if left.cell_starts.len() != right.cell_starts.len() {
            return false;
        }

        let tolerance = left.font_size.max(right.font_size).max(10.0) * 1.4;
        left.cell_starts
            .iter()
            .zip(&right.cell_starts)
            .all(|(lhs, rhs)| (lhs - rhs).abs() <= tolerance)
    }

    fn should_break_paragraph(
        previous_bbox: [f32; 4],
        line: &LineDescriptor,
        body_font_size: f32,
    ) -> bool {
        let vertical_gap = (previous_bbox[1] - line.bbox[3]).max(0.0);
        let indent_shift = (line.bbox[0] - previous_bbox[0]).abs();
        vertical_gap > body_font_size * 1.7 || indent_shift > body_font_size * 2.8
    }

    fn classify_layout_region(
        bbox: [f32; 4],
        page_width: f32,
        left_boundary: f32,
        right_boundary: f32,
        two_column_detected: bool,
    ) -> LayoutRegion {
        let width = (bbox[2] - bbox[0]).max(0.0);
        let center_x = (bbox[0] + bbox[2]) / 2.0;
        if !two_column_detected || width >= page_width * 0.72 {
            return LayoutRegion::Span;
        }
        if center_x <= left_boundary {
            LayoutRegion::Left
        } else if center_x >= right_boundary {
            LayoutRegion::Right
        } else {
            LayoutRegion::Span
        }
    }

    fn apply_layout_regions(lines: &mut [LineDescriptor], page_width: f32) {
        let narrow_lines: Vec<&LineDescriptor> = lines
            .iter()
            .filter(|line| (line.bbox[2] - line.bbox[0]) < page_width * 0.72)
            .collect();
        if narrow_lines.len() < 4 {
            for line in lines {
                line.region = LayoutRegion::Span;
            }
            return;
        }

        let mut left_centers: Vec<f32> = narrow_lines
            .iter()
            .map(|line| (line.bbox[0] + line.bbox[2]) / 2.0)
            .filter(|center| *center < page_width * 0.5)
            .collect();
        let mut right_centers: Vec<f32> = narrow_lines
            .iter()
            .map(|line| (line.bbox[0] + line.bbox[2]) / 2.0)
            .filter(|center| *center >= page_width * 0.5)
            .collect();
        if left_centers.len() < 2 || right_centers.len() < 2 {
            for line in lines {
                line.region = LayoutRegion::Span;
            }
            return;
        }

        left_centers.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        right_centers.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let left_median = left_centers[left_centers.len() / 2];
        let right_median = right_centers[right_centers.len() / 2];
        let separation = right_median - left_median;
        let two_column_detected = separation >= page_width * 0.18;

        for line in lines {
            line.region = Self::classify_layout_region(
                line.bbox,
                page_width,
                left_median + separation * 0.18,
                right_median - separation * 0.18,
                two_column_detected,
            );
        }
    }

    fn order_lines_for_page(mut lines: Vec<LineDescriptor>) -> Vec<LineDescriptor> {
        lines.sort_by(|a, b| {
            b.bbox[3]
                .partial_cmp(&a.bbox[3])
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut ordered = Vec::with_capacity(lines.len());
        let mut column_segment: Vec<LineDescriptor> = Vec::new();

        let flush_segment = |ordered: &mut Vec<LineDescriptor>,
                             segment: &mut Vec<LineDescriptor>| {
            if segment.is_empty() {
                return;
            }
            let mut left: Vec<LineDescriptor> = Vec::new();
            let mut right: Vec<LineDescriptor> = Vec::new();
            let mut span: Vec<LineDescriptor> = Vec::new();
            for line in std::mem::take(segment) {
                match line.region {
                    LayoutRegion::Left => left.push(line),
                    LayoutRegion::Right => right.push(line),
                    LayoutRegion::Span => span.push(line),
                }
            }
            left.sort_by(|a, b| {
                b.bbox[3]
                    .partial_cmp(&a.bbox[3])
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            right.sort_by(|a, b| {
                b.bbox[3]
                    .partial_cmp(&a.bbox[3])
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            span.sort_by(|a, b| {
                b.bbox[3]
                    .partial_cmp(&a.bbox[3])
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            ordered.extend(span);
            ordered.extend(left);
            ordered.extend(right);
        };

        for line in lines {
            if line.region == LayoutRegion::Span {
                flush_segment(&mut ordered, &mut column_segment);
                ordered.push(line);
            } else {
                column_segment.push(line);
            }
        }
        flush_segment(&mut ordered, &mut column_segment);
        ordered
    }

    fn estimate_table_column_bounding_boxes(
        lines: &[LineDescriptor],
        expected_columns: usize,
        anchors: &[f32],
    ) -> Vec<[f32; 4]> {
        if lines.is_empty() || expected_columns == 0 {
            return Vec::new();
        }

        let table_left = lines
            .iter()
            .map(|line| line.bbox[0])
            .fold(f32::MAX, f32::min);
        let table_bottom = lines
            .iter()
            .map(|line| line.bbox[1])
            .fold(f32::MAX, f32::min);
        let table_right = lines
            .iter()
            .map(|line| line.bbox[2])
            .fold(0.0_f32, f32::max);
        let table_top = lines
            .iter()
            .map(|line| line.bbox[3])
            .fold(0.0_f32, f32::max);

        let mut starts = vec![0.0_f32; expected_columns];
        for idx in 0..expected_columns {
            starts[idx] = if anchors.get(idx).copied().unwrap_or(0.0) > 0.0 {
                anchors[idx]
            } else if idx == 0 {
                table_left
            } else {
                let width = (table_right - table_left).max(1.0) / expected_columns as f32;
                table_left + width * idx as f32
            };
        }

        for idx in 1..expected_columns {
            if starts[idx] <= starts[idx - 1] {
                starts[idx] = starts[idx - 1]
                    + ((table_right - table_left).max(1.0) / expected_columns as f32);
            }
        }

        let mut boxes = Vec::with_capacity(expected_columns);
        for idx in 0..expected_columns {
            let left = if idx == 0 {
                table_left
            } else {
                starts[idx].min(table_right)
            };
            let right = if idx + 1 < expected_columns {
                ((starts[idx] + starts[idx + 1]) / 2.0).max(left)
            } else {
                table_right
            };
            boxes.push([
                left.max(table_left),
                table_bottom,
                right.min(table_right),
                table_top,
            ]);
        }
        boxes
    }

    fn normalize_table_structure(lines: &[LineDescriptor]) -> NormalizedTable {
        if lines.is_empty() {
            return NormalizedTable {
                headers: None,
                rows: Vec::new(),
                column_bounding_boxes: Vec::new(),
            };
        }

        let mut counts = std::collections::BTreeMap::<usize, usize>::new();
        for line in lines {
            let count = if !line.cells.is_empty() {
                line.cells.len()
            } else {
                Self::split_table_row(&line.text).map_or(1, |cells| cells.len())
            };
            *counts.entry(count).or_insert(0) += 1;
        }

        let expected_columns = counts
            .into_iter()
            .max_by_key(|(_, freq)| *freq)
            .map(|(count, _)| count)
            .unwrap_or(1)
            .max(1);

        let mut anchors = vec![0.0_f32; expected_columns];
        let mut anchor_hits = vec![0usize; expected_columns];
        for line in lines
            .iter()
            .filter(|line| line.cell_starts.len() == expected_columns)
        {
            for (idx, start) in line.cell_starts.iter().enumerate() {
                anchors[idx] += *start;
                anchor_hits[idx] += 1;
            }
        }
        for (anchor, hits) in anchors.iter_mut().zip(anchor_hits.iter()) {
            if *hits > 0 {
                *anchor /= *hits as f32;
            }
        }

        let column_bounding_boxes =
            Self::estimate_table_column_bounding_boxes(lines, expected_columns, &anchors);

        let mut rows: Vec<Vec<String>> = Vec::new();
        for line in lines {
            let raw_cells = if !line.cells.is_empty() {
                line.cells.clone()
            } else {
                Self::split_table_row(&line.text).unwrap_or_else(|| vec![line.text.clone()])
            };

            let normalized = if raw_cells.len() == expected_columns {
                raw_cells
            } else {
                let mut row = vec![String::new(); expected_columns];
                if raw_cells.len() == 1 {
                    if let Some(previous) = rows.last_mut() {
                        let target = previous
                            .iter()
                            .enumerate()
                            .max_by_key(|(_, value)| value.len())
                            .map(|(idx, _)| idx)
                            .unwrap_or(expected_columns.saturating_sub(1));
                        if !previous[target].is_empty() {
                            previous[target].push(' ');
                        }
                        previous[target].push_str(raw_cells[0].trim());
                        continue;
                    }
                    row[0] = raw_cells[0].clone();
                } else {
                    for (cell_idx, cell) in raw_cells.into_iter().enumerate() {
                        let target = line
                            .cell_starts
                            .get(cell_idx)
                            .and_then(|start| {
                                anchors
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, anchor)| **anchor > 0.0)
                                    .min_by(|(_, a), (_, b)| {
                                        (start - **a)
                                            .abs()
                                            .partial_cmp(&(start - **b).abs())
                                            .unwrap_or(std::cmp::Ordering::Equal)
                                    })
                                    .map(|(idx, _)| idx)
                            })
                            .unwrap_or(cell_idx.min(expected_columns - 1));
                        if !row[target].is_empty() {
                            row[target].push(' ');
                        }
                        row[target].push_str(cell.trim());
                    }
                }
                row
            };

            rows.push(normalized);
        }

        let mut headers = None;
        if let Some(first_row) = rows.first() {
            if Self::is_header_like_row(first_row) {
                headers = Some(first_row.clone());
                rows.remove(0);
            } else if rows.len() >= 2 && Self::is_header_like_row(&rows[1]) {
                headers = Some(rows.remove(1));
            }
        }

        NormalizedTable {
            headers,
            rows,
            column_bounding_boxes,
        }
    }

    fn is_header_like_row(row: &[String]) -> bool {
        if row.is_empty() {
            return false;
        }

        let non_empty: Vec<&str> = row
            .iter()
            .map(|cell| cell.trim())
            .filter(|cell| !cell.is_empty())
            .collect();
        if non_empty.is_empty() {
            return false;
        }

        let short_cells = non_empty
            .iter()
            .filter(|cell| cell.chars().count() <= 24)
            .count();
        let non_numeric = non_empty
            .iter()
            .filter(|cell| {
                !cell.chars().all(|ch| {
                    ch.is_ascii_digit()
                        || matches!(ch, '.' | ',' | '%' | '-' | '+' | '(' | ')' | '$')
                })
            })
            .count();
        let titleish = non_empty
            .iter()
            .filter(|cell| {
                Self::looks_like_heading(cell)
                    || cell.chars().next().map(char::is_uppercase).unwrap_or(false)
            })
            .count();

        short_cells * 2 >= non_empty.len()
            && non_numeric * 2 >= non_empty.len()
            && titleish * 2 >= non_empty.len()
    }

    fn same_region_flow(left: LayoutRegion, right: LayoutRegion) -> bool {
        left == right || left == LayoutRegion::Span || right == LayoutRegion::Span
    }

    fn build_lines_from_chunks(
        &self,
        mut chunks: Vec<PositionedTextChunk>,
        extracted_text: &str,
        page_width: f32,
        page_height: f32,
    ) -> Vec<LineDescriptor> {
        if chunks.is_empty() {
            return extracted_text
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(|line| LineDescriptor {
                    text: line.to_string(),
                    bbox: [
                        72.0,
                        page_height - 96.0,
                        (page_width - 72.0).max(72.0),
                        page_height - 72.0,
                    ],
                    font_size: 10.0,
                    cells: Self::split_table_row(line).unwrap_or_default(),
                    cell_starts: Vec::new(),
                    region: LayoutRegion::Span,
                })
                .collect();
        }

        chunks.sort_by(|a, b| {
            let by = b.y.partial_cmp(&a.y).unwrap_or(std::cmp::Ordering::Equal);
            if by == std::cmp::Ordering::Equal {
                a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal)
            } else {
                by
            }
        });

        let mut lines: Vec<Vec<PositionedTextChunk>> = Vec::new();
        for chunk in chunks {
            if let Some(current_line) = lines.last_mut() {
                let reference_y = current_line[0].y;
                let tolerance = current_line
                    .iter()
                    .map(|item| item.font_size)
                    .fold(0.0_f32, f32::max)
                    .max(chunk.font_size)
                    * 0.6;
                if (reference_y - chunk.y).abs() <= tolerance.max(4.0) {
                    current_line.push(chunk);
                    continue;
                }
            }
            lines.push(vec![chunk]);
        }

        let mut lines: Vec<LineDescriptor> = lines
            .into_iter()
            .map(|mut line_chunks| {
                line_chunks
                    .sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
                let left = line_chunks
                    .iter()
                    .map(|c| c.x)
                    .fold(page_width, f32::min)
                    .max(0.0);
                let bottom = line_chunks
                    .iter()
                    .map(|c| c.bottom)
                    .fold(page_height, f32::min)
                    .max(0.0);
                let right = line_chunks
                    .iter()
                    .map(|c| c.right)
                    .fold(0.0_f32, f32::max)
                    .min(page_width);
                let top = line_chunks
                    .iter()
                    .map(|c| c.top)
                    .fold(0.0_f32, f32::max)
                    .min(page_height);
                let font_size =
                    line_chunks.iter().map(|c| c.font_size).sum::<f32>() / line_chunks.len() as f32;
                let (cells, cell_starts) = Self::line_cells_and_starts_from_chunks(&line_chunks);
                let text = if cells.len() >= 3 {
                    cells.join(" | ")
                } else {
                    let mut out = String::new();
                    let mut previous_right: Option<f32> = None;
                    for chunk in &line_chunks {
                        if let Some(prev_right) = previous_right {
                            let gap = (chunk.x - prev_right).max(0.0);
                            if gap >= font_size.max(10.0) * 0.8 {
                                out.push(' ');
                            }
                        }
                        out.push_str(chunk.text.trim());
                        previous_right = Some(chunk.right);
                    }
                    out
                };

                LineDescriptor {
                    text: text.trim().to_string(),
                    bbox: [left, bottom, right, top],
                    font_size,
                    cells,
                    cell_starts,
                    region: LayoutRegion::Span,
                }
            })
            .filter(|line| !line.text.is_empty())
            .collect();
        Self::apply_layout_regions(&mut lines, page_width);
        lines
    }

    fn classify_line_roles(lines: &[LineDescriptor], body_font_size: f32) -> Vec<LineRole> {
        lines
            .iter()
            .map(|line| {
                if Self::line_is_table_candidate(line) {
                    LineRole::Table
                } else if line.font_size >= body_font_size * 1.18
                    || (line.font_size >= body_font_size * 1.05
                        && Self::looks_like_heading(&line.text))
                {
                    LineRole::Heading
                } else {
                    LineRole::Body
                }
            })
            .collect()
    }

    fn lines_to_elements(
        &self,
        mut lines: Vec<LineDescriptor>,
        page_idx: usize,
    ) -> Vec<PdfElement> {
        if lines.is_empty() {
            return Vec::new();
        }

        lines = Self::order_lines_for_page(lines);

        let body_font_size = {
            let mut sizes: Vec<f32> = lines.iter().map(|line| line.font_size).collect();
            sizes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            sizes[sizes.len() / 2].max(10.0)
        };
        let roles = Self::classify_line_roles(&lines, body_font_size);

        let mut elements = Vec::new();
        let mut paragraph_text = String::new();
        let mut paragraph_bbox: Option<[f32; 4]> = None;
        let mut paragraph_font = body_font_size;
        let mut paragraph_region: Option<LayoutRegion> = None;
        let mut pending_table: Vec<LineDescriptor> = Vec::new();

        let flush_paragraph = |elements: &mut Vec<PdfElement>,
                               paragraph_text: &mut String,
                               paragraph_bbox: &mut Option<[f32; 4]>,
                               paragraph_font: f32,
                               page_idx: usize| {
            if let Some(bbox) = paragraph_bbox.take() {
                if !paragraph_text.trim().is_empty() {
                    elements.push(PdfElement::Paragraph {
                        content: paragraph_text.trim().to_string(),
                        font_size: paragraph_font,
                        page_number: page_idx,
                        bounding_box: bbox,
                    });
                }
            }
            paragraph_text.clear();
        };

        let flush_table = |elements: &mut Vec<PdfElement>,
                           pending_table: &mut Vec<LineDescriptor>,
                           page_idx: usize| {
            if pending_table.is_empty() {
                return;
            }
            let left = pending_table
                .iter()
                .map(|line| line.bbox[0])
                .fold(f32::MAX, f32::min);
            let bottom = pending_table
                .iter()
                .map(|line| line.bbox[1])
                .fold(f32::MAX, f32::min);
            let right = pending_table
                .iter()
                .map(|line| line.bbox[2])
                .fold(0.0_f32, f32::max);
            let top = pending_table
                .iter()
                .map(|line| line.bbox[3])
                .fold(0.0_f32, f32::max);
            let table = Self::normalize_table_structure(pending_table);
            elements.push(PdfElement::Table {
                headers: table.headers,
                rows: table.rows,
                column_bounding_boxes: Some(table.column_bounding_boxes),
                page_number: page_idx,
                bounding_box: [left, bottom, right, top],
            });
            pending_table.clear();
        };

        let spill_pending_table_into_paragraph =
            |pending_table: &mut Vec<LineDescriptor>,
             elements: &mut Vec<PdfElement>,
             paragraph_text: &mut String,
             paragraph_bbox: &mut Option<[f32; 4]>,
             paragraph_font: &mut f32,
             page_idx: usize| {
                if pending_table.is_empty() {
                    return;
                }
                let drained = std::mem::take(pending_table);
                for line in drained {
                    if paragraph_text.is_empty() {
                        *paragraph_font = line.font_size;
                        *paragraph_bbox = Some(line.bbox);
                        paragraph_text.push_str(&line.text);
                    } else {
                        if let Some(bbox) = paragraph_bbox.as_mut() {
                            bbox[0] = bbox[0].min(line.bbox[0]);
                            bbox[1] = bbox[1].min(line.bbox[1]);
                            bbox[2] = bbox[2].max(line.bbox[2]);
                            bbox[3] = bbox[3].max(line.bbox[3]);
                        }
                        if !paragraph_text.ends_with('-') {
                            paragraph_text.push(' ');
                        }
                        paragraph_text.push_str(&line.text);
                    }
                }
                if !paragraph_text.is_empty() {
                    flush_paragraph(
                        elements,
                        paragraph_text,
                        paragraph_bbox,
                        *paragraph_font,
                        page_idx,
                    );
                }
            };

        for (line, role) in lines.into_iter().zip(roles.into_iter()) {
            if role == LineRole::Table {
                flush_paragraph(
                    &mut elements,
                    &mut paragraph_text,
                    &mut paragraph_bbox,
                    paragraph_font,
                    page_idx,
                );
                paragraph_region = None;

                if let Some(previous) = pending_table.last() {
                    if !Self::same_region_flow(previous.region, line.region)
                        || !Self::lines_share_column_alignment(previous, &line)
                    {
                        if pending_table.len() >= 2 {
                            flush_table(&mut elements, &mut pending_table, page_idx);
                        } else {
                            spill_pending_table_into_paragraph(
                                &mut pending_table,
                                &mut elements,
                                &mut paragraph_text,
                                &mut paragraph_bbox,
                                &mut paragraph_font,
                                page_idx,
                            );
                        }
                    }
                }
                pending_table.push(line);
                continue;
            }

            if pending_table.len() >= 2 {
                flush_table(&mut elements, &mut pending_table, page_idx);
            } else {
                spill_pending_table_into_paragraph(
                    &mut pending_table,
                    &mut elements,
                    &mut paragraph_text,
                    &mut paragraph_bbox,
                    &mut paragraph_font,
                    page_idx,
                );
                if paragraph_bbox.is_some() && paragraph_region.is_none() {
                    paragraph_region = Some(LayoutRegion::Span);
                }
            }

            if role == LineRole::Heading {
                flush_paragraph(
                    &mut elements,
                    &mut paragraph_text,
                    &mut paragraph_bbox,
                    paragraph_font,
                    page_idx,
                );
                paragraph_region = None;
                elements.push(PdfElement::Heading {
                    content: line.text,
                    level: if line.font_size >= body_font_size * 1.45 {
                        1
                    } else {
                        2
                    },
                    page_number: page_idx,
                    bounding_box: line.bbox,
                });
                continue;
            }

            if paragraph_text.is_empty() {
                paragraph_font = line.font_size;
                paragraph_bbox = Some(line.bbox);
                paragraph_region = Some(line.region);
                paragraph_text.push_str(&line.text);
            } else {
                let should_break = paragraph_bbox
                    .map(|bbox| Self::should_break_paragraph(bbox, &line, body_font_size))
                    .unwrap_or(false);
                let region_break = paragraph_region
                    .map(|region| !Self::same_region_flow(region, line.region))
                    .unwrap_or(false);
                if should_break || region_break {
                    flush_paragraph(
                        &mut elements,
                        &mut paragraph_text,
                        &mut paragraph_bbox,
                        paragraph_font,
                        page_idx,
                    );
                    paragraph_font = line.font_size;
                    paragraph_bbox = Some(line.bbox);
                    paragraph_region = Some(line.region);
                    paragraph_text.push_str(&line.text);
                    continue;
                }
                if let Some(bbox) = paragraph_bbox.as_mut() {
                    bbox[0] = bbox[0].min(line.bbox[0]);
                    bbox[1] = bbox[1].min(line.bbox[1]);
                    bbox[2] = bbox[2].max(line.bbox[2]);
                    bbox[3] = bbox[3].max(line.bbox[3]);
                }
                if !paragraph_text.ends_with('-') {
                    paragraph_text.push(' ');
                }
                paragraph_text.push_str(&line.text);
            }
        }

        if pending_table.len() >= 2 {
            flush_table(&mut elements, &mut pending_table, page_idx);
        } else {
            spill_pending_table_into_paragraph(
                &mut pending_table,
                &mut elements,
                &mut paragraph_text,
                &mut paragraph_bbox,
                &mut paragraph_font,
                page_idx,
            );
        }

        flush_paragraph(
            &mut elements,
            &mut paragraph_text,
            &mut paragraph_bbox,
            paragraph_font,
            page_idx,
        );
        elements
    }

    async fn parse_page_via_ai(
        &self,
        page_num: usize,
        page_image: &image::DynamicImage,
        mut triage: PdfPageTriage,
        mut triage_reasons: Vec<String>,
        ocr_backend_name: &str,
        ocr_language: &str,
    ) -> anyhow::Result<PageParseResult> {
        let (result, selected_route, fallback_used) = match self
            .try_vlm_parse(page_image, page_num)
            .await
        {
            Ok(res) => (res, "page_image_vlm".to_string(), None),
            Err(e) => {
                warn!(
                    "VLM failed on page {} ({}). Falling back to OCR...",
                    page_num, e
                );
                triage_reasons.push("vlm_failed".to_string());
                match self
                    .try_ocr_parse(page_image, ocr_backend_name, ocr_language)
                    .await
                {
                    Ok(res) => (res, "page_image_ocr".to_string(), Some("vlm".to_string())),
                    Err(_) => (
                        self.call_text_extract_fallback(page_image, ocr_backend_name, ocr_language)
                            .await?,
                        "page_image_text_extract".to_string(),
                        Some("vlm>ocr".to_string()),
                    ),
                }
            }
        };

        let elements = Self::structured_elements_from_text_with_layout(
            &result,
            page_num,
            page_image.width() as f32,
            page_image.height() as f32,
        );
        triage.native_line_count = elements.len();

        let degraded = fallback_used.is_some();
        let (source_contract_kind, source_contract_ref) =
            page_source_contract(page_num, &selected_route);
        Ok(PageParseResult {
            route: PdfPageRoute {
                page_number: page_num,
                selected_route,
                text_layer_detected: false,
                fallback_used,
                source_contract_kind,
                source_contract_ref,
                triage_reasons,
                degraded,
                triage,
            },
            elements,
        })
    }

    async fn try_vlm_parse(
        &self,
        page_image: &image::DynamicImage,
        _page_num: usize,
    ) -> anyhow::Result<String> {
        let prompt =
            "Extract all content from this page in Markdown format. Preserve tables and formulas.";
        let res = self
            .sensory
            .vision_check(page_image.clone(), Some(prompt), self.model.as_deref())
            .await?;
        match res {
            SensoryOutput::Text(text) => Ok(text),
            SensoryOutput::Coordinates { x, y, label } => Ok(format!(
                "Point of interest at [{x}, {y}]{}",
                label.map(|value| format!(" - {value}")).unwrap_or_default()
            )),
            other => Err(anyhow::anyhow!(
                "Unsupported sensory output for PDF VLM parsing: {:?}",
                other
            )),
        }
    }

    async fn try_ocr_parse(
        &self,
        page_image: &image::DynamicImage,
        backend_name: &str,
        language: &str,
    ) -> anyhow::Result<String> {
        if backend_name.eq_ignore_ascii_case("auto") || backend_name.trim().is_empty() {
            if let Ok(output) = self
                .sensory
                .vision_check(page_image.clone(), None, Some("global-ocr"))
                .await
            {
                if let SensoryOutput::Text(text) = output {
                    if !text.trim().is_empty() {
                        return Ok(text);
                    }
                }
            }
        }

        let ocr_backend: Arc<dyn benshu_inference::backend::OcrBackend> =
            if backend_name.eq_ignore_ascii_case("tesseract") {
                Arc::new(
                    benshu_inference::backend::ocr_tesseract::TesseractBackend::new(
                        language.to_string(),
                    ),
                )
            } else {
                benshu_inference::backend::InferenceFactory::create_ocr_backend(
                    std::path::Path::new(backend_name),
                )
                .await?
            };
        ocr_backend
            .recognize(page_image)
            .await
            .map_err(|e| anyhow::anyhow!("OCR fallback failed: {}", e))
    }

    async fn call_text_extract_fallback(
        &self,
        page_image: &image::DynamicImage,
        backend_name: &str,
        language: &str,
    ) -> anyhow::Result<String> {
        self.try_ocr_parse(page_image, backend_name, language).await
    }

    fn element_sort_key(element: &PdfElement) -> (usize, f32, f32, f32) {
        match element {
            PdfElement::Paragraph {
                page_number,
                bounding_box,
                ..
            }
            | PdfElement::Heading {
                page_number,
                bounding_box,
                ..
            }
            | PdfElement::Table {
                page_number,
                bounding_box,
                ..
            }
            | PdfElement::Image {
                page_number,
                bounding_box,
                ..
            } => (
                *page_number,
                bounding_box[3],
                bounding_box[0],
                bounding_box[1],
            ),
        }
    }

    /// Preserve page ordering and use bounding boxes for an honest best-effort reading order.
    fn sort_by_reading_order(elements: &mut Vec<PdfElement>) {
        elements.sort_by(|a, b| {
            let (ap, atop, aleft, abottom) = Self::element_sort_key(a);
            let (bp, btop, bleft, bbottom) = Self::element_sort_key(b);
            ap.cmp(&bp)
                .then_with(|| btop.partial_cmp(&atop).unwrap_or(std::cmp::Ordering::Equal))
                .then_with(|| {
                    aleft
                        .partial_cmp(&bleft)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| {
                    bbottom
                        .partial_cmp(&abottom)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
        });
    }

    fn next_enriched_bbox(
        cursor_top: &mut f32,
        page_width: f32,
        page_height: f32,
        block_height: f32,
        inset: f32,
    ) -> [f32; 4] {
        let left = inset.clamp(24.0, page_width - 48.0);
        let right = (page_width - inset).max(left + 24.0);
        let top = (*cursor_top).clamp(48.0, page_height.max(48.0));
        let bottom = (top - block_height).max(24.0);
        *cursor_top = (bottom - 14.0).max(24.0);
        [left, bottom, right, top]
    }

    fn estimated_table_columns(
        headers: Option<&[String]>,
        rows: &[Vec<String>],
        bbox: [f32; 4],
    ) -> Option<Vec<[f32; 4]>> {
        let columns = headers
            .map(|cells| cells.len())
            .or_else(|| rows.iter().map(Vec::len).max())
            .unwrap_or(0);
        if columns < 2 {
            return None;
        }
        let width = (bbox[2] - bbox[0]).max(1.0) / columns as f32;
        Some(
            (0..columns)
                .map(|idx| {
                    let left = bbox[0] + width * idx as f32;
                    let right = if idx + 1 == columns {
                        bbox[2]
                    } else {
                        left + width
                    };
                    [left, bbox[1], right, bbox[3]]
                })
                .collect(),
        )
    }

    fn structured_elements_from_text_with_layout(
        content: &str,
        page_num: usize,
        page_width: f32,
        page_height: f32,
    ) -> Vec<PdfElement> {
        let mut elements = Vec::new();
        let mut paragraph = Vec::new();
        let mut table_rows: Vec<Vec<String>> = Vec::new();
        let mut table_has_header_separator = false;
        let mut cursor_top = (page_height - 36.0).max(36.0);

        let flush_paragraph = |elements: &mut Vec<PdfElement>,
                               paragraph: &mut Vec<String>,
                               cursor_top: &mut f32| {
            if paragraph.is_empty() {
                return;
            }
            let content = paragraph.join(" ").trim().to_string();
            paragraph.clear();
            if !content.is_empty() {
                let lines = ((content.chars().count().max(1) as f32 / 92.0).ceil() as usize).max(1);
                let bbox = Self::next_enriched_bbox(
                    cursor_top,
                    page_width,
                    page_height,
                    18.0 * lines as f32,
                    72.0,
                );
                elements.push(PdfElement::Paragraph {
                    content,
                    font_size: 10.0,
                    page_number: page_num,
                    bounding_box: bbox,
                });
            }
        };

        let flush_table = |elements: &mut Vec<PdfElement>,
                           table_rows: &mut Vec<Vec<String>>,
                           table_has_header_separator: &mut bool,
                           cursor_top: &mut f32| {
            if table_rows.is_empty() {
                *table_has_header_separator = false;
                return;
            }
            let mut headers = None;
            let rows = if *table_has_header_separator && !table_rows.is_empty() {
                headers = Some(table_rows.remove(0));
                table_rows.clone()
            } else {
                table_rows.clone()
            };
            let row_count = rows.len() + usize::from(headers.is_some());
            let bbox = Self::next_enriched_bbox(
                cursor_top,
                page_width,
                page_height,
                22.0 + 18.0 * row_count.max(1) as f32,
                72.0,
            );
            let column_bounding_boxes =
                Self::estimated_table_columns(headers.as_deref(), &rows, bbox);
            elements.push(PdfElement::Table {
                headers,
                rows,
                column_bounding_boxes,
                page_number: page_num,
                bounding_box: bbox,
            });
            table_rows.clear();
            *table_has_header_separator = false;
        };

        for raw_line in content.lines() {
            let line = raw_line.trim();
            if line.is_empty() {
                flush_paragraph(&mut elements, &mut paragraph, &mut cursor_top);
                flush_table(
                    &mut elements,
                    &mut table_rows,
                    &mut table_has_header_separator,
                    &mut cursor_top,
                );
                continue;
            }

            if let Some((level, heading)) = Self::parse_markdown_heading(line) {
                flush_paragraph(&mut elements, &mut paragraph, &mut cursor_top);
                flush_table(
                    &mut elements,
                    &mut table_rows,
                    &mut table_has_header_separator,
                    &mut cursor_top,
                );
                let lines = ((heading.chars().count().max(1) as f32 / 60.0).ceil() as usize).max(1);
                let bbox = Self::next_enriched_bbox(
                    &mut cursor_top,
                    page_width,
                    page_height,
                    26.0 + 16.0 * lines.saturating_sub(1) as f32,
                    72.0,
                );
                elements.push(PdfElement::Heading {
                    content: heading.to_string(),
                    level,
                    page_number: page_num,
                    bounding_box: bbox,
                });
                continue;
            }

            if Self::is_markdown_table_separator(line) {
                table_has_header_separator = true;
                continue;
            }

            if let Some(cells) = Self::parse_markdown_table_row(line) {
                flush_paragraph(&mut elements, &mut paragraph, &mut cursor_top);
                table_rows.push(cells);
                continue;
            }

            flush_table(
                &mut elements,
                &mut table_rows,
                &mut table_has_header_separator,
                &mut cursor_top,
            );
            paragraph.push(line.to_string());
        }

        flush_paragraph(&mut elements, &mut paragraph, &mut cursor_top);
        flush_table(
            &mut elements,
            &mut table_rows,
            &mut table_has_header_separator,
            &mut cursor_top,
        );

        if elements.is_empty() {
            let bbox =
                Self::next_enriched_bbox(&mut cursor_top, page_width, page_height, 18.0, 72.0);
            elements.push(PdfElement::Paragraph {
                content: content.trim().to_string(),
                font_size: 10.0,
                page_number: page_num,
                bounding_box: bbox,
            });
        }

        elements
    }

    #[cfg(test)]
    fn structured_elements_from_text(content: &str, page_num: usize) -> Vec<PdfElement> {
        Self::structured_elements_from_text_with_layout(content, page_num, 612.0, 792.0)
    }

    fn parse_markdown_heading(line: &str) -> Option<(usize, &str)> {
        let trimmed = line.trim_start();
        let hashes = trimmed.chars().take_while(|ch| *ch == '#').count();
        if hashes == 0 || hashes > 6 {
            return None;
        }
        let heading = trimmed[hashes..].trim_start();
        (!heading.is_empty()).then_some((hashes, heading))
    }

    fn parse_markdown_table_row(line: &str) -> Option<Vec<String>> {
        if !line.contains('|') {
            return None;
        }
        let cells: Vec<String> = line
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .map(ToOwned::to_owned)
            .collect();
        (cells.iter().any(|cell| !cell.is_empty()) && cells.len() >= 2).then_some(cells)
    }

    fn is_markdown_table_separator(line: &str) -> bool {
        line.contains('|')
            && line.trim_matches('|').split('|').all(|cell| {
                let cell = cell.trim();
                !cell.is_empty() && cell.chars().all(|ch| matches!(ch, '-' | ':' | ' '))
            })
    }

    fn artifact_base_dir() -> PathBuf {
        std::env::temp_dir().join("benshu-pdf-artifacts")
    }

    fn cleanup_stale_artifact_roots() -> anyhow::Result<()> {
        let base_dir = Self::artifact_base_dir();
        if !base_dir.exists() {
            return Ok(());
        }
        let now = std::time::SystemTime::now();
        for entry in std::fs::read_dir(&base_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let metadata = entry.metadata()?;
            let modified = metadata.modified().unwrap_or(now);
            let age = now
                .duration_since(modified)
                .unwrap_or_else(|_| Duration::from_secs(0));
            if age.as_secs() >= ARTIFACT_RETENTION_SECS {
                let _ = std::fs::remove_dir_all(&path);
            }
        }
        Ok(())
    }

    fn artifact_root(pdf_path: &str) -> anyhow::Result<PathBuf> {
        let stem = Path::new(pdf_path)
            .file_stem()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("document");
        let root = Self::artifact_base_dir().join(format!("{}-{}", stem, Uuid::new_v4()));
        std::fs::create_dir_all(&root)?;
        Ok(root)
    }

    fn build_image_source(
        image: &image::DynamicImage,
        image_format: image::ImageFormat,
        artifact_root: Option<&Path>,
        page_idx: usize,
        image_name: &str,
        image_output: PdfImageOutputMode,
    ) -> anyhow::Result<Option<String>> {
        match image_output {
            PdfImageOutputMode::Off => Ok(None),
            PdfImageOutputMode::Embedded => {
                let (bytes, mime) = Self::encode_image_bytes(image, image_format)?;
                Ok(Some(format!(
                    "data:{};base64,{}",
                    mime,
                    base64::engine::general_purpose::STANDARD.encode(bytes)
                )))
            }
            PdfImageOutputMode::Artifact => {
                let root = artifact_root.ok_or_else(|| {
                    anyhow::anyhow!(
                        "artifact image output requested without an active artifact session"
                    )
                })?;
                let ext = match image_format {
                    image::ImageFormat::Jpeg => "jpg",
                    _ => "png",
                };
                let filename = format!("page-{}-{}.{}", page_idx, image_name, ext);
                let path = root.join(filename);
                image.save_with_format(&path, image_format)?;
                Ok(Some(path.to_string_lossy().to_string()))
            }
        }
    }

    fn encode_image_bytes(
        image: &image::DynamicImage,
        image_format: image::ImageFormat,
    ) -> anyhow::Result<(Vec<u8>, &'static str)> {
        let mut cursor = std::io::Cursor::new(Vec::new());
        image.write_to(&mut cursor, image_format)?;
        let mime = match image_format {
            image::ImageFormat::Jpeg => "image/jpeg",
            _ => "image/png",
        };
        Ok((cursor.into_inner(), mime))
    }

    fn stream_filter_names(stream: &lopdf::Stream) -> Vec<String> {
        stream
            .dict
            .get(b"Filter")
            .ok()
            .map(|filter| match filter {
                lopdf::Object::Array(array) => array
                    .iter()
                    .filter_map(|obj| obj.as_name().ok())
                    .map(|name| String::from_utf8_lossy(name).to_string())
                    .collect(),
                lopdf::Object::Name(name) => vec![String::from_utf8_lossy(name).to_string()],
                _ => Vec::new(),
            })
            .unwrap_or_default()
    }

    fn decode_stream_image(
        stream: &lopdf::Stream,
    ) -> anyhow::Result<(image::DynamicImage, image::ImageFormat)> {
        let filters = Self::stream_filter_names(stream);
        if filters
            .iter()
            .any(|filter| filter == "DCTDecode" || filter == "JPXDecode")
        {
            let format = image::guess_format(&stream.content).unwrap_or(image::ImageFormat::Jpeg);
            let image = image::load_from_memory(&stream.content)?;
            return Ok((image, format));
        }

        let width = stream.dict.get(b"Width")?.as_i64()? as u32;
        let height = stream.dict.get(b"Height")?.as_i64()? as u32;
        let bits_per_component = stream
            .dict
            .get(b"BitsPerComponent")
            .ok()
            .and_then(|obj| obj.as_i64().ok())
            .unwrap_or(8);
        if bits_per_component != 8 {
            anyhow::bail!(
                "unsupported image bits per component for in-process decode: {}",
                bits_per_component
            );
        }

        let decoded = stream.decompressed_content()?;
        let color_space = stream
            .dict
            .get(b"ColorSpace")
            .ok()
            .and_then(|obj| match obj {
                lopdf::Object::Name(name) => Some(String::from_utf8_lossy(name).to_string()),
                lopdf::Object::Array(array) => array
                    .first()
                    .and_then(|value| value.as_name().ok())
                    .map(|name| String::from_utf8_lossy(name).to_string()),
                _ => None,
            });

        let image = match color_space.as_deref() {
            Some("DeviceGray") => {
                let buffer = image::GrayImage::from_raw(width, height, decoded)
                    .ok_or_else(|| anyhow::anyhow!("invalid grayscale PDF image buffer"))?;
                image::DynamicImage::ImageLuma8(buffer)
            }
            Some("DeviceRGB") | None => {
                let buffer = image::RgbImage::from_raw(width, height, decoded)
                    .ok_or_else(|| anyhow::anyhow!("invalid RGB PDF image buffer"))?;
                image::DynamicImage::ImageRgb8(buffer)
            }
            other => {
                anyhow::bail!(
                    "unsupported PDF image colorspace for in-process decode: {:?}",
                    other
                )
            }
        };

        Ok((image, image::ImageFormat::Png))
    }

    fn is_page_dominant_image(
        image: &image::DynamicImage,
        page_width: f32,
        page_height: f32,
        image_count: usize,
    ) -> bool {
        if image_count == 0 {
            return false;
        }
        let page_aspect = (page_width / page_height.max(1.0)).max(0.01);
        let image_aspect = (image.width() as f32 / image.height().max(1) as f32).max(0.01);
        let aspect_delta = (page_aspect.ln() - image_aspect.ln()).abs();
        let area = image.width().saturating_mul(image.height());
        image_count == 1 && aspect_delta <= 0.35 && area >= 512 * 512
    }

    fn extract_page_images(
        &self,
        doc: &LopdfDocument,
        page_id: lopdf::ObjectId,
        page_idx: usize,
        page_width: f32,
        page_height: f32,
        artifact_session: &mut PdfArtifactSession,
        image_output: PdfImageOutputMode,
    ) -> anyhow::Result<PageImageSet> {
        let (resources_opt, _) = doc.get_page_resources(page_id);
        let Some(resources) = resources_opt else {
            return Ok(PageImageSet {
                elements: Vec::new(),
                dominant_image: None,
                warnings: Vec::new(),
            });
        };
        let Ok(xobjects) = resources.get(b"XObject") else {
            return Ok(PageImageSet {
                elements: Vec::new(),
                dominant_image: None,
                warnings: Vec::new(),
            });
        };
        let xobjects_dict = if let Ok(dict) = xobjects.as_dict() {
            Some(dict)
        } else if let Ok(reference) = xobjects.as_reference() {
            doc.get_dictionary(reference).ok()
        } else {
            None
        };
        let Some(xobjects_dict) = xobjects_dict else {
            return Ok(PageImageSet {
                elements: Vec::new(),
                dominant_image: None,
                warnings: Vec::new(),
            });
        };
        let artifact_store = ControlledPdfArtifactStore;

        let mut decoded_images: Vec<(String, image::DynamicImage, image::ImageFormat)> = Vec::new();
        let mut warnings = Vec::new();
        for (name, obj) in xobjects_dict {
            let Ok(stream) = doc
                .get_object(obj.as_reference()?)
                .and_then(|object| object.as_stream())
            else {
                continue;
            };
            if !stream
                .dict
                .get(b"Subtype")
                .map_or(false, |value| value.as_name().unwrap_or(b"") == b"Image")
            {
                continue;
            }

            match Self::decode_stream_image(stream) {
                Ok((image, format)) => {
                    decoded_images.push((String::from_utf8_lossy(name).to_string(), image, format))
                }
                Err(error) => {
                    warn!(
                        "Skipping unsupported PDF image on page {}: {}",
                        page_idx, error
                    );
                    warnings.push(format!("page_{}_unsupported_image_decode", page_idx));
                }
            }
        }

        let mut elements = Vec::new();
        let mut dominant_image = None;
        for (name, image, format) in decoded_images {
            let page_dominant = Self::is_page_dominant_image(&image, page_width, page_height, 1);
            if page_dominant && dominant_image.is_none() {
                let Some(source) = artifact_store.build_image_source(
                    &image,
                    format,
                    artifact_session.root(),
                    page_idx,
                    &name,
                    image_output,
                )?
                else {
                    dominant_image = Some(image);
                    continue;
                };
                dominant_image = Some(image.clone());
                elements.push(PdfElement::Image {
                    source,
                    page_number: page_idx,
                    bounding_box: [0.0, 0.0, page_width, page_height],
                });
                artifact_session.record_emitted_artifact();
            }
        }

        Ok(PageImageSet {
            elements,
            dominant_image,
            warnings,
        })
    }

    fn parse_native_page(
        &self,
        page_idx: usize,
        extraction: NativePageExtraction,
        mut triage: PdfPageTriage,
        selected_route: String,
        fallback_used: Option<String>,
        triage_reasons: Vec<String>,
        mut image_elements: Vec<PdfElement>,
    ) -> anyhow::Result<PageParseResult> {
        let lines = self.build_lines_from_chunks(
            extraction.chunks,
            &extraction.extracted_text,
            extraction.page_width,
            extraction.page_height,
        );
        triage.native_line_count = lines.len();
        let text_layer_detected = matches!(triage.text_layer_quality, PdfTextLayerQuality::Strong);
        let degraded = fallback_used.is_some()
            || !matches!(triage.text_layer_quality, PdfTextLayerQuality::Strong);
        let mut elements = self.lines_to_elements(lines, page_idx);
        elements.append(&mut image_elements);
        let (source_contract_kind, source_contract_ref) =
            page_source_contract(page_idx, &selected_route);

        Ok(PageParseResult {
            route: PdfPageRoute {
                page_number: page_idx,
                selected_route,
                text_layer_detected,
                fallback_used,
                source_contract_kind,
                source_contract_ref,
                triage_reasons,
                degraded,
                triage,
            },
            elements,
        })
    }

    fn degraded_page_result(
        page_idx: usize,
        selected_route: impl Into<String>,
        fallback_used: Option<String>,
        mut triage_reasons: Vec<String>,
        triage: PdfPageTriage,
        summary: impl Into<String>,
    ) -> PageParseResult {
        triage_reasons.push("page_degraded".to_string());
        let summary = summary.into();
        let selected_route = selected_route.into();
        let (source_contract_kind, source_contract_ref) =
            page_source_contract(page_idx, &selected_route);
        PageParseResult {
            route: PdfPageRoute {
                page_number: page_idx,
                selected_route,
                text_layer_detected: matches!(
                    triage.text_layer_quality,
                    PdfTextLayerQuality::Strong
                ),
                fallback_used,
                source_contract_kind,
                source_contract_ref,
                triage_reasons,
                degraded: true,
                triage,
            },
            elements: vec![PdfElement::Paragraph {
                content: format!("[pdf_parse degraded] {}", summary),
                font_size: 10.0,
                page_number: page_idx,
                bounding_box: [72.0, 680.0, 540.0, 720.0],
            }],
        }
    }

    fn record_page_result(
        metrics: &mut PdfParseMetrics,
        page_routes: &mut Vec<PdfPageRoute>,
        elements: &mut Vec<PdfElement>,
        page_result: PageParseResult,
    ) {
        metrics.pages_processed += 1;
        if page_result.route.degraded {
            metrics.degraded_pages += 1;
        }
        if page_result.route.fallback_used.is_some() {
            metrics.fallback_pages += 1;
        }
        if page_result
            .route
            .selected_route
            .contains("native_structured")
        {
            metrics.native_pages += 1;
        } else {
            metrics.enriched_pages += 1;
        }
        page_routes.push(page_result.route);
        elements.extend(page_result.elements);
    }

    fn build_route_report(
        warnings: &[String],
        page_routes: &[PdfPageRoute],
        large_document_guard_active: bool,
    ) -> PdfRouteReport {
        let encrypted = warnings
            .iter()
            .any(|warning| warning == "pdf_encrypted_or_password_protected");
        let mut unsupported_features = std::collections::BTreeSet::new();
        for warning in warnings {
            if warning.contains("unsupported") {
                unsupported_features.insert(warning.clone());
            }
        }
        let degraded_pages = page_routes
            .iter()
            .filter(|route| route.degraded)
            .map(|route| route.page_number)
            .collect();

        PdfRouteReport {
            encrypted,
            large_document_guard_active,
            unsupported_features: unsupported_features.into_iter().collect(),
            degraded_pages,
        }
    }

    fn apply_large_document_guard(
        total_pages: usize,
        target_pages: &Option<Vec<usize>>,
        page_limit: usize,
        warnings: &mut Vec<String>,
    ) -> anyhow::Result<bool> {
        let large_document_guard_active = total_pages > page_limit;
        if target_pages.is_none() && large_document_guard_active {
            anyhow::bail!(
                "pdf_parse refused to process {} pages without an explicit page selection; page_limit={}",
                total_pages,
                page_limit
            );
        }
        if large_document_guard_active {
            warnings.push(format!(
                "large_document_guard_active_total_pages_{}_page_limit_{}",
                total_pages, page_limit
            ));
        }
        Ok(large_document_guard_active)
    }

    async fn parse_document(
        &self,
        path: &str,
        mode: &str,
        target_pages: &Option<Vec<usize>>,
        image_output: PdfImageOutputMode,
        ocr_backend_name: &str,
        ocr_language: &str,
        page_limit: usize,
    ) -> anyhow::Result<PdfOutput> {
        let doc = LopdfDocument::load(path).map_err(|e| anyhow::anyhow!("lopdf failed: {}", e))?;
        let native_extractor = LopdfNativeExtractor;
        let page_triage = HeuristicPdfPageTriage;
        let page_enricher = SensoryStructuredEnricher;
        let page_rasterizer = InProcessPageRasterizer { image_output };
        let mut artifact_session = PdfArtifactSession::new(path, image_output)?;
        let (title, author) = Self::extract_metadata(&doc);
        let normalized_mode = Self::normalize_mode(mode);
        let total_pages = doc.get_pages().len();
        let mut warnings = Vec::new();
        if Self::document_is_encrypted(&doc) {
            warnings.push("pdf_encrypted_or_password_protected".to_string());
        }
        let large_document_guard_active =
            Self::apply_large_document_guard(total_pages, target_pages, page_limit, &mut warnings)?;
        let mut elements = Vec::new();
        let mut page_routes = Vec::new();
        let mut metrics = PdfParseMetrics {
            pages_processed: 0,
            degraded_pages: 0,
            native_pages: 0,
            enriched_pages: 0,
            fallback_pages: 0,
            dominant_image_pages: 0,
        };

        for (page_num, page_id) in doc.get_pages() {
            let page_idx = page_num as usize;
            if let Some(target) = target_pages {
                if !target.contains(&page_idx) {
                    continue;
                }
            }

            let extraction = match native_extractor.extract_page(self, &doc, page_num, page_id) {
                Ok(extraction) => extraction,
                Err(error) => {
                    warnings.push(format!("page_{}_native_extract_failed", page_idx));
                    let page_result = Self::degraded_page_result(
                        page_idx,
                        "native_extract_error",
                        Some("native_extract".to_string()),
                        vec!["native_extraction_failed".to_string()],
                        PdfPageTriage {
                            text_layer_quality: PdfTextLayerQuality::None,
                            text_chars: 0,
                            alnum_chars: 0,
                            native_line_count: 0,
                            dominant_page_image_detected: false,
                            rotation_degrees: 0,
                        },
                        format!("native extraction failed: {}", error),
                    );
                    Self::record_page_result(
                        &mut metrics,
                        &mut page_routes,
                        &mut elements,
                        page_result,
                    );
                    continue;
                }
            };
            let rotation_degrees = Self::page_rotation_degrees(&doc, page_id);
            let page_images = match page_rasterizer.rasterize_page(
                self,
                &doc,
                page_id,
                page_idx,
                extraction.page_width,
                extraction.page_height,
                &mut artifact_session,
            ) {
                Ok(page_images) => page_images,
                Err(error) => {
                    warnings.push(format!("page_{}_rasterize_failed", page_idx));
                    warn!(
                        "Page rasterization failed on page {} ({}). Continuing without page image enrichment.",
                        page_idx, error
                    );
                    PageImageSet {
                        elements: Vec::new(),
                        dominant_image: None,
                        warnings: Vec::new(),
                    }
                }
            };
            let (triage, route_decision) = page_triage.decide(
                normalized_mode,
                &extraction.extracted_text,
                page_images.dominant_image.is_some(),
                rotation_degrees,
            );
            let triage = triage;
            if rotation_degrees != 0 {
                warnings.push(format!("page_{}_rotation_{}", page_idx, rotation_degrees));
            }
            if triage.dominant_page_image_detected {
                metrics.dominant_image_pages += 1;
            }
            let extracted_text = extraction.extracted_text.clone();
            let PageImageSet {
                elements: page_image_elements,
                dominant_image,
                warnings: page_image_warnings,
            } = page_images;
            warnings.extend(page_image_warnings);
            let selected_route = route_decision.selected_route.clone();
            let triage_reasons = route_decision.triage_reasons.clone();
            let triage_for_page = triage.clone();
            let triage_reasons_for_page = triage_reasons.clone();
            let page_future = async move {
                if route_decision.use_native {
                    self.parse_native_page(
                        page_idx,
                        extraction,
                        triage_for_page,
                        selected_route,
                        None,
                        triage_reasons_for_page,
                        page_image_elements,
                    )
                } else {
                    match dominant_image.as_ref() {
                        Some(page_image) => match page_enricher
                            .enrich_page(
                                self,
                                page_idx,
                                page_image,
                                triage_for_page.clone(),
                                triage_reasons_for_page.clone(),
                                ocr_backend_name,
                                ocr_language,
                            )
                            .await
                        {
                            Ok(result) => Ok(result),
                            Err(error)
                                if (normalized_mode == "auto" || normalized_mode == "hybrid")
                                    && !extracted_text.trim().is_empty() =>
                            {
                                warn!(
                                    "Structured image parsing failed on page {} ({}). Falling back to native text layer.",
                                    page_idx, error
                                );
                                let mut fallback_reasons = triage_reasons_for_page.clone();
                                fallback_reasons
                                    .push("structured_page_image_failed".to_string());
                                self.parse_native_page(
                                    page_idx,
                                    extraction,
                                    triage_for_page,
                                    "native_structured".to_string(),
                                    Some("structured_page_image".to_string()),
                                    fallback_reasons,
                                    page_image_elements,
                                )
                            }
                            Err(error) => Err(error),
                        },
                        None if (normalized_mode == "auto" || normalized_mode == "hybrid")
                            && !extracted_text.trim().is_empty() =>
                        {
                            let mut fallback_reasons = triage_reasons_for_page.clone();
                            fallback_reasons
                                .push("no_in_process_page_image_available".to_string());
                            self.parse_native_page(
                                page_idx,
                                extraction,
                                triage_for_page,
                                "native_structured".to_string(),
                                Some("no_in_process_page_image".to_string()),
                                fallback_reasons,
                                page_image_elements,
                            )
                        }
                        None => Err(anyhow::anyhow!(
                            "No in-process page image available for structured image parsing on page {}",
                            page_idx
                        )),
                    }
                }
            };
            let page_result = match tokio::time::timeout(
                std::time::Duration::from_secs(PAGE_ROUTE_TIMEOUT_SECS),
                page_future,
            )
            .await
            {
                Ok(Ok(result)) => result,
                Ok(Err(error)) => {
                    warnings.push(format!("page_{}_route_failed", page_idx));
                    Self::degraded_page_result(
                        page_idx,
                        "page_route_degraded",
                        Some("route_error".to_string()),
                        triage_reasons.clone(),
                        triage.clone(),
                        format!("page routing failed: {}", error),
                    )
                }
                Err(_) => {
                    warnings.push(format!("page_{}_timeout", page_idx));
                    Self::degraded_page_result(
                        page_idx,
                        "page_route_timeout",
                        Some("timeout".to_string()),
                        triage_reasons.clone(),
                        triage.clone(),
                        format!("page timed out after {}s", PAGE_ROUTE_TIMEOUT_SECS),
                    )
                }
            };

            Self::record_page_result(&mut metrics, &mut page_routes, &mut elements, page_result);
        }

        Self::sort_by_reading_order(&mut elements);
        let artifact_root = artifact_session.finalize()?;
        let route_report =
            Self::build_route_report(&warnings, &page_routes, large_document_guard_active);
        Ok(PdfOutput {
            title,
            author,
            total_pages,
            parser_mode: normalized_mode.to_string(),
            image_output: match image_output {
                PdfImageOutputMode::Off => "off".to_string(),
                PdfImageOutputMode::Embedded => "embedded".to_string(),
                PdfImageOutputMode::Artifact => "artifact".to_string(),
            },
            artifact_root,
            artifact_cleanup_policy: format!(
                "artifact directories are created per parse run, empty roots are removed immediately, and stale roots older than {}s are pruned on the next artifact run",
                ARTIFACT_RETENTION_SECS
            ),
            ocr_backend: ocr_backend_name.to_string(),
            ocr_language: ocr_language.to_string(),
            warnings,
            route_report,
            metrics,
            page_routes,
            elements,
        })
    }
}

#[derive(Deserialize)]
struct PdfParseArgs {
    path: String,
    #[serde(default = "default_mode")]
    mode: String, // "auto", "text", "vision", "hybrid"
    #[serde(default = "default_format")]
    format: String, // "json", "markdown"
    #[serde(default = "default_image_output")]
    image_output: String, // "off", "embedded", "artifact"
    #[serde(default = "default_ocr_backend")]
    ocr_backend: String,
    #[serde(default = "default_ocr_language")]
    ocr_language: String,
    #[serde(default = "default_page_limit")]
    page_limit: usize,
    #[serde(default)]
    pages: Option<Vec<usize>>,
}

fn default_mode() -> String {
    "auto".to_string()
}
fn default_format() -> String {
    "markdown".to_string()
}
fn default_image_output() -> String {
    "off".to_string()
}
fn default_ocr_backend() -> String {
    "auto".to_string()
}
fn default_ocr_language() -> String {
    "eng".to_string()
}
fn default_page_limit() -> usize {
    DEFAULT_PAGE_LIMIT
}

#[async_trait]
impl Tool for PdfParseTool {
    fn name(&self) -> String {
        "pdf_parse".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "pdf_parse".to_string(),
            description: "Advanced PDF parser inspired by OpenDataLoader. Extracts text, structure, metadata and tables.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Local path to the PDF file." },
                    "mode": { "type": "string", "enum": ["auto", "text", "vision", "hybrid"], "description": "Parsing strategy. 'auto' prefers native text layers and uses in-process page-image enrichment when a page is scan-like. 'hybrid' enables the same structured routing but prefers image enrichment more aggressively on image-dominant pages." },
                    "format": { "type": "string", "enum": ["json", "markdown"], "description": "Output format. Markdown is recommended for LLM consumption." },
                    "image_output": { "type": "string", "enum": ["off", "embedded", "artifact"], "description": "How extracted page images should be exposed. 'off' skips image artifacts, 'embedded' returns data URIs, 'artifact' writes into a controlled temp artifact directory." },
                    "ocr_backend": { "type": "string", "description": "OCR backend for image-dominant pages. Defaults to the globally configured OCR route; explicit values may request a specific backend such as 'tesseract'." },
                    "ocr_language": { "type": "string", "description": "OCR language hint such as 'eng' or 'chi_sim'. Defaults to 'eng'." },
                    "page_limit": { "type": "integer", "description": "Maximum pages allowed without explicit page selection. Defaults to 400." },
                    "pages": { "type": "array", "items": { "type": "integer" }, "description": "Specific pages to parse." }
                },
                "required": ["path"]
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use this to extract structured PDF content. 'auto' is the default and routes each page through native structured parsing or in-process page-image enrichment depending on text-layer quality and page-image availability.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: PdfParseArgs = serde_json::from_str(arguments)
            .map_err(|e| anyhow::anyhow!("Tool arguments error for pdf_parse: {}", e))?;

        if !Path::new(&args.path).exists() {
            return Ok(json!({"error": format!("File not found: {}", args.path)}).to_string());
        }

        let image_output = Self::normalize_image_output(Some(&args.image_output));
        let output = match tokio::time::timeout(
            std::time::Duration::from_secs(120),
            self.parse_document(
                &args.path,
                &args.mode,
                &args.pages,
                image_output,
                &args.ocr_backend,
                &args.ocr_language,
                args.page_limit,
            ),
        )
        .await
        {
            Ok(result) => result?,
            Err(_) => {
                warn!("pdf_parse timed out after 120s for {}.", args.path);
                return Ok(json!({
                    "error": format!("pdf_parse timed out after 120s for {}", args.path)
                })
                .to_string());
            }
        };

        if args.format == "markdown" {
            Ok(Self::to_markdown(&output))
        } else {
            Ok(serde_json::to_string_pretty(&output)?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_table_row_handles_pipe_and_spaced_rows() {
        assert_eq!(
            PdfParseTool::split_table_row("Name | Score | Rank"),
            Some(vec!["Name".into(), "Score".into(), "Rank".into()])
        );
        assert_eq!(
            PdfParseTool::split_table_row("Name  Score  Rank"),
            Some(vec!["Name".into(), "Score".into(), "Rank".into()])
        );
    }

    #[test]
    fn normalize_mode_maps_hybrid_to_auto() {
        assert_eq!(PdfParseTool::normalize_mode("hybrid"), "hybrid");
        assert_eq!(PdfParseTool::normalize_mode("vision"), "vision");
    }

    #[test]
    fn decide_page_route_records_image_dominance_for_hybrid() {
        let triage = PdfPageTriage {
            text_layer_quality: PdfTextLayerQuality::Weak,
            text_chars: 18,
            alnum_chars: 12,
            native_line_count: 0,
            dominant_page_image_detected: true,
            rotation_degrees: 0,
        };

        let decision = PdfParseTool::decide_page_route("hybrid", &triage);
        assert_eq!(decision.selected_route, "native_structured_with_enrichment");
        assert!(!decision.use_native);
        assert!(decision
            .triage_reasons
            .contains(&"dominant_page_image_detected".to_string()));
    }

    #[test]
    fn structured_elements_from_text_preserves_headings_and_tables() {
        let elements = PdfParseTool::structured_elements_from_text(
            "# Intro\n\nAlpha beta\n\n| Name | Score |\n| --- | --- |\n| Alice | 98 |",
            1,
        );
        assert!(matches!(elements[0], PdfElement::Heading { .. }));
        assert!(matches!(elements[1], PdfElement::Paragraph { .. }));
        assert!(matches!(elements[2], PdfElement::Table { .. }));
    }

    #[test]
    fn structured_elements_from_text_with_layout_assigns_page_local_bounding_boxes() {
        let elements = PdfParseTool::structured_elements_from_text_with_layout(
            "# Intro\n\nAlpha beta gamma\n\n| Name | Score |\n| --- | --- |\n| Alice | 98 |",
            2,
            600.0,
            900.0,
        );
        assert!(elements.iter().all(|element| match element {
            PdfElement::Paragraph { bounding_box, .. }
            | PdfElement::Heading { bounding_box, .. }
            | PdfElement::Table { bounding_box, .. }
            | PdfElement::Image { bounding_box, .. } => {
                bounding_box[2] > bounding_box[0] && bounding_box[3] > bounding_box[1]
            }
        }));
        assert!(matches!(
            elements[2],
            PdfElement::Table {
                column_bounding_boxes: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn markdown_output_preserves_title_and_tables() {
        let output = PdfOutput {
            title: Some("Demo".into()),
            author: None,
            total_pages: 1,
            parser_mode: "auto".into(),
            image_output: "off".into(),
            artifact_root: None,
            artifact_cleanup_policy: "none".into(),
            ocr_backend: "auto".into(),
            ocr_language: "eng".into(),
            warnings: Vec::new(),
            route_report: PdfRouteReport {
                encrypted: false,
                large_document_guard_active: false,
                unsupported_features: Vec::new(),
                degraded_pages: Vec::new(),
            },
            metrics: PdfParseMetrics {
                pages_processed: 1,
                degraded_pages: 0,
                native_pages: 1,
                enriched_pages: 0,
                fallback_pages: 0,
                dominant_image_pages: 0,
            },
            page_routes: vec![PdfPageRoute {
                page_number: 1,
                selected_route: "native_structured".into(),
                text_layer_detected: true,
                fallback_used: None,
                source_contract_kind: None,
                source_contract_ref: None,
                triage_reasons: vec!["strong_text_layer_detected".into()],
                degraded: false,
                triage: PdfPageTriage {
                    text_layer_quality: PdfTextLayerQuality::Strong,
                    text_chars: 120,
                    alnum_chars: 96,
                    native_line_count: 2,
                    dominant_page_image_detected: false,
                    rotation_degrees: 0,
                },
            }],
            elements: vec![
                PdfElement::Heading {
                    content: "HEADER".into(),
                    level: 2,
                    page_number: 1,
                    bounding_box: [0.0, 0.0, 0.0, 0.0],
                },
                PdfElement::Table {
                    headers: None,
                    rows: vec![vec!["A".into(), "B".into()], vec!["1".into(), "2".into()]],
                    column_bounding_boxes: None,
                    page_number: 1,
                    bounding_box: [0.0, 0.0, 0.0, 0.0],
                },
            ],
        };

        let markdown = PdfParseTool::to_markdown(&output);
        assert!(markdown.contains("# Demo"));
        assert!(markdown.contains("## HEADER"));
        assert!(markdown.contains("| A | B |"));
    }

    #[test]
    fn artifact_session_removes_empty_root_on_finalize() {
        let session =
            PdfArtifactSession::new("/tmp/demo.pdf", PdfImageOutputMode::Artifact).unwrap();
        let root = session.root().unwrap().to_path_buf();
        assert!(root.exists());
        let finalized = session.finalize().unwrap();
        assert!(finalized.is_none());
        assert!(!root.exists());
    }

    #[test]
    fn large_document_guard_requires_explicit_page_selection() {
        let mut warnings = Vec::new();
        let error =
            PdfParseTool::apply_large_document_guard(12, &None, 5, &mut warnings).unwrap_err();
        assert!(error
            .to_string()
            .contains("refused to process 12 pages without an explicit page selection"));
        assert!(warnings.is_empty());

        let selected_pages = Some(vec![1, 2]);
        let active =
            PdfParseTool::apply_large_document_guard(12, &selected_pages, 5, &mut warnings)
                .unwrap();
        assert!(active);
        assert_eq!(
            warnings,
            vec!["large_document_guard_active_total_pages_12_page_limit_5"]
        );
    }

    #[test]
    fn route_report_surfaces_encrypted_unsupported_and_degraded_pages() {
        let warnings = vec![
            "pdf_encrypted_or_password_protected".to_string(),
            "page_2_unsupported_image_decode".to_string(),
            "page_2_unsupported_image_decode".to_string(),
        ];
        let page_routes = vec![
            PdfPageRoute {
                page_number: 1,
                selected_route: "native_structured".into(),
                text_layer_detected: true,
                fallback_used: None,
                source_contract_kind: None,
                source_contract_ref: None,
                triage_reasons: vec!["strong_text_layer_detected".into()],
                degraded: false,
                triage: PdfPageTriage {
                    text_layer_quality: PdfTextLayerQuality::Strong,
                    text_chars: 10,
                    alnum_chars: 8,
                    native_line_count: 1,
                    dominant_page_image_detected: false,
                    rotation_degrees: 0,
                },
            },
            PdfPageRoute {
                page_number: 2,
                selected_route: "page_route_degraded".into(),
                text_layer_detected: false,
                fallback_used: Some("route_error".into()),
                source_contract_kind: Some("pdf_page_image".into()),
                source_contract_ref: Some("pdf_page:2".into()),
                triage_reasons: vec!["page_degraded".into()],
                degraded: true,
                triage: PdfPageTriage {
                    text_layer_quality: PdfTextLayerQuality::None,
                    text_chars: 0,
                    alnum_chars: 0,
                    native_line_count: 0,
                    dominant_page_image_detected: true,
                    rotation_degrees: 0,
                },
            },
        ];

        let report = PdfParseTool::build_route_report(&warnings, &page_routes, true);
        assert!(report.encrypted);
        assert!(report.large_document_guard_active);
        assert_eq!(
            report.unsupported_features,
            vec!["page_2_unsupported_image_decode"]
        );
        assert_eq!(report.degraded_pages, vec![2]);
    }

    #[test]
    fn page_source_contract_marks_pdf_page_image_routes() {
        let (kind, reference) = page_source_contract(3, "page_image_ocr");
        assert_eq!(kind.as_deref(), Some("pdf_page_image"));
        assert_eq!(reference.as_deref(), Some("pdf_page:3"));

        let (kind, reference) = page_source_contract(1, "native_structured");
        assert!(kind.is_none());
        assert!(reference.is_none());
    }

    #[test]
    fn sort_by_reading_order_uses_page_and_bounding_box() {
        let mut elements = vec![
            PdfElement::Paragraph {
                content: "page two".into(),
                font_size: 10.0,
                page_number: 2,
                bounding_box: [72.0, 500.0, 300.0, 520.0],
            },
            PdfElement::Paragraph {
                content: "right first".into(),
                font_size: 10.0,
                page_number: 1,
                bounding_box: [320.0, 640.0, 520.0, 660.0],
            },
            PdfElement::Paragraph {
                content: "left first".into(),
                font_size: 10.0,
                page_number: 1,
                bounding_box: [72.0, 640.0, 252.0, 660.0],
            },
            PdfElement::Heading {
                content: "top heading".into(),
                level: 1,
                page_number: 1,
                bounding_box: [72.0, 700.0, 520.0, 720.0],
            },
        ];

        PdfParseTool::sort_by_reading_order(&mut elements);

        let ordered: Vec<&str> = elements
            .iter()
            .map(|element| match element {
                PdfElement::Paragraph { content, .. } | PdfElement::Heading { content, .. } => {
                    content.as_str()
                }
                PdfElement::Table { .. } | PdfElement::Image { .. } => "other",
            })
            .collect();
        assert_eq!(
            ordered,
            vec!["top heading", "left first", "right first", "page two"]
        );
    }

    #[test]
    fn line_cells_from_chunks_splits_columns_by_gap() {
        let chunks = vec![
            PositionedTextChunk {
                text: "Name".into(),
                x: 72.0,
                y: 700.0,
                right: 104.0,
                bottom: 690.0,
                top: 710.0,
                font_size: 10.0,
            },
            PositionedTextChunk {
                text: "Score".into(),
                x: 190.0,
                y: 700.0,
                right: 236.0,
                bottom: 690.0,
                top: 710.0,
                font_size: 10.0,
            },
            PositionedTextChunk {
                text: "Rank".into(),
                x: 320.0,
                y: 700.0,
                right: 356.0,
                bottom: 690.0,
                top: 710.0,
                font_size: 10.0,
            },
        ];

        assert_eq!(
            PdfParseTool::line_cells_and_starts_from_chunks(&chunks).0,
            vec!["Name".to_string(), "Score".to_string(), "Rank".to_string()]
        );
    }

    #[test]
    fn aligned_lines_are_detected_as_same_table_shape() {
        let left = LineDescriptor {
            text: "Name | Score | Rank".into(),
            bbox: [72.0, 680.0, 360.0, 700.0],
            font_size: 10.0,
            cells: vec!["Name".into(), "Score".into(), "Rank".into()],
            cell_starts: vec![72.0, 190.0, 320.0],
            region: LayoutRegion::Left,
        };
        let right = LineDescriptor {
            text: "Alice | 98 | 1".into(),
            bbox: [72.0, 660.0, 360.0, 680.0],
            font_size: 10.0,
            cells: vec!["Alice".into(), "98".into(), "1".into()],
            cell_starts: vec![74.0, 188.0, 322.0],
            region: LayoutRegion::Left,
        };

        assert!(PdfParseTool::lines_share_column_alignment(&left, &right));
    }

    #[test]
    fn paragraph_break_detects_large_vertical_gap() {
        let line = LineDescriptor {
            text: "Next paragraph".into(),
            bbox: [72.0, 540.0, 300.0, 556.0],
            font_size: 10.0,
            cells: Vec::new(),
            cell_starts: Vec::new(),
            region: LayoutRegion::Span,
        };

        assert!(PdfParseTool::should_break_paragraph(
            [72.0, 580.0, 320.0, 596.0],
            &line,
            10.0,
        ));
    }

    #[test]
    fn normalize_table_rows_merges_wrapped_row_fragments() {
        let table = PdfParseTool::normalize_table_structure(&[
            LineDescriptor {
                text: "Name | Notes | Score".into(),
                bbox: [72.0, 700.0, 360.0, 720.0],
                font_size: 10.0,
                cells: vec!["Name".into(), "Notes".into(), "Score".into()],
                cell_starts: vec![72.0, 180.0, 320.0],
                region: LayoutRegion::Left,
            },
            LineDescriptor {
                text: "Alice | Long explanation that wraps".into(),
                bbox: [72.0, 680.0, 320.0, 700.0],
                font_size: 10.0,
                cells: vec!["Alice".into(), "Long explanation that wraps".into()],
                cell_starts: vec![72.0, 180.0],
                region: LayoutRegion::Left,
            },
            LineDescriptor {
                text: "onto another line".into(),
                bbox: [180.0, 664.0, 320.0, 680.0],
                font_size: 10.0,
                cells: vec!["onto another line".into()],
                cell_starts: vec![180.0],
                region: LayoutRegion::Left,
            },
        ]);

        assert_eq!(
            table.headers,
            Some(vec!["Name".into(), "Notes".into(), "Score".into()])
        );
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0][0], "Alice");
        assert!(table.rows[0][1].contains("onto another line"));
        assert_eq!(table.column_bounding_boxes.len(), 3);
    }

    #[test]
    fn header_like_row_detection_prefers_short_named_columns() {
        assert!(PdfParseTool::is_header_like_row(&[
            "Name".into(),
            "Invoice Date".into(),
            "Total".into(),
        ]));
        assert!(!PdfParseTool::is_header_like_row(&[
            "alice@example.com".into(),
            "2026-03-22".into(),
            "193.40".into(),
        ]));
    }

    #[test]
    fn order_lines_for_page_prefers_left_column_before_right_column() {
        let ordered = PdfParseTool::order_lines_for_page(vec![
            LineDescriptor {
                text: "Right 1".into(),
                bbox: [340.0, 640.0, 520.0, 660.0],
                font_size: 10.0,
                cells: Vec::new(),
                cell_starts: Vec::new(),
                region: LayoutRegion::Right,
            },
            LineDescriptor {
                text: "Left 1".into(),
                bbox: [72.0, 640.0, 252.0, 660.0],
                font_size: 10.0,
                cells: Vec::new(),
                cell_starts: Vec::new(),
                region: LayoutRegion::Left,
            },
            LineDescriptor {
                text: "Span heading".into(),
                bbox: [72.0, 700.0, 520.0, 720.0],
                font_size: 16.0,
                cells: Vec::new(),
                cell_starts: Vec::new(),
                region: LayoutRegion::Span,
            },
        ]);

        assert_eq!(ordered[0].text, "Span heading");
        assert_eq!(ordered[1].text, "Left 1");
        assert_eq!(ordered[2].text, "Right 1");
    }

    #[test]
    fn lines_to_elements_does_not_merge_across_columns() {
        let tool = PdfParseTool::new(
            None,
            None,
            Arc::new(SensoryHub::new(
                benshu_sensory::hub::SensoryConfig::default(),
            )),
        );
        let elements = tool.lines_to_elements(
            vec![
                LineDescriptor {
                    text: "Left column first paragraph".into(),
                    bbox: [72.0, 640.0, 252.0, 660.0],
                    font_size: 10.0,
                    cells: Vec::new(),
                    cell_starts: Vec::new(),
                    region: LayoutRegion::Left,
                },
                LineDescriptor {
                    text: "Right column first paragraph".into(),
                    bbox: [340.0, 640.0, 520.0, 660.0],
                    font_size: 10.0,
                    cells: Vec::new(),
                    cell_starts: Vec::new(),
                    region: LayoutRegion::Right,
                },
            ],
            1,
        );

        assert_eq!(elements.len(), 2);
        match (&elements[0], &elements[1]) {
            (
                PdfElement::Paragraph { content: left, .. },
                PdfElement::Paragraph { content: right, .. },
            ) => {
                assert_eq!(left, "Left column first paragraph");
                assert_eq!(right, "Right column first paragraph");
            }
            other => panic!("expected paragraph elements, got {:?}", other),
        }
    }

    #[test]
    fn normalize_table_structure_extracts_headers_and_column_boxes() {
        let table = PdfParseTool::normalize_table_structure(&[
            LineDescriptor {
                text: "Item | Qty | Price".into(),
                bbox: [72.0, 700.0, 360.0, 720.0],
                font_size: 10.0,
                cells: vec!["Item".into(), "Qty".into(), "Price".into()],
                cell_starts: vec![72.0, 190.0, 300.0],
                region: LayoutRegion::Span,
            },
            LineDescriptor {
                text: "Keyboard | 2 | 199".into(),
                bbox: [72.0, 680.0, 360.0, 700.0],
                font_size: 10.0,
                cells: vec!["Keyboard".into(), "2".into(), "199".into()],
                cell_starts: vec![72.0, 190.0, 300.0],
                region: LayoutRegion::Span,
            },
        ]);

        assert_eq!(
            table.headers,
            Some(vec!["Item".into(), "Qty".into(), "Price".into()])
        );
        assert_eq!(
            table.rows,
            vec![vec![
                String::from("Keyboard"),
                String::from("2"),
                String::from("199"),
            ]]
        );
        assert_eq!(table.column_bounding_boxes.len(), 3);
        assert!(table.column_bounding_boxes[0][0] <= 72.0);
        assert!(table.column_bounding_boxes[2][2] >= 360.0);
    }
}
