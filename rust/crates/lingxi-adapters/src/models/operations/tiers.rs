//! The OpenAI image resolution-tier resolver (R05-T06): a rule-for-rule
//! port of the incumbent `core/media-adapters/resolution-tiers.ts`
//! (`resolveOpenAiImageSize` and its helpers) — standard (dall-e-3-class)
//! and flexible (gpt-image-2-class) sizing, the tier vocabulary
//! (1K/2K/4K/auto), and the exact refusal texts.

use lingxi_protocol::ProtocolError;

use super::parse;

pub const OPENAI_MIN_PIXELS: u64 = 655_360;
pub const OPENAI_MAX_PIXELS: u64 = 8_294_400;
pub const OPENAI_MAX_EDGE: u32 = 3_840;
pub const OPENAI_MAX_RATIO: u64 = 3;

/// The incumbent `OPENAI_IMAGE_RATIOS` vocabulary (order = declaration).
pub const OPENAI_IMAGE_RATIOS: [&str; 8] =
    ["1:1", "4:3", "3:4", "16:9", "9:16", "3:2", "2:3", "21:9"];
pub const OPENAI_STANDARD_IMAGE_RATIOS: [&str; 3] = ["1:1", "3:2", "2:3"];
pub const OPENAI_FLEXIBLE_IMAGE_RATIOS: [&str; 8] = OPENAI_IMAGE_RATIOS;
pub const OPENAI_STANDARD_RESOLUTION_TIERS: [&str; 1] = ["1K"];
pub const OPENAI_FLEXIBLE_RESOLUTION_TIERS: [&str; 3] = ["1K", "2K", "4K"];
pub const CODEX_IMAGE_RESOLUTION_TIERS: [&str; 2] = ["1K", "2K"];

const TIER_LONG_EDGE: [(&str, u32); 3] = [("1k", 1024), ("2k", 2048), ("4k", 3840)];
const STANDARD_SIZES: [&str; 3] = ["1024x1024", "1536x1024", "1024x1536"];
const STANDARD_SIZES_BY_RATIO: [(&str, &str); 3] = [
    ("1:1", "1024x1024"),
    ("3:2", "1536x1024"),
    ("2:3", "1024x1536"),
];

/// The resolver's per-call options (the incumbent `options` bag). A
/// `None`/`Default` field reproduces the incumbent's own fallback
/// (`flexible !== false`, ratio/tier vocabularies by flexibility, no
/// constraint overrides).
#[derive(Debug, Clone, Copy)]
pub struct OpenAiSizeOptions<'a> {
    pub source_name: &'a str,
    pub flexible: bool,
    pub supported_ratios: Option<&'a [&'a str]>,
    pub supported_resolutions: Option<&'a [&'a str]>,
    pub default_ratio: Option<&'a str>,
    pub default_resolution: Option<&'a str>,
    pub constraints: FlexibleConstraints,
}

impl<'a> Default for OpenAiSizeOptions<'a> {
    fn default() -> Self {
        Self {
            source_name: "OpenAI image",
            flexible: true,
            supported_ratios: None,
            supported_resolutions: None,
            default_ratio: None,
            default_resolution: None,
            constraints: FlexibleConstraints::default(),
        }
    }
}

/// The incumbent `constraints` bag (`constraints.maxEdge || OPENAI_MAX_EDGE`
/// — a zero override falls back to the constant, exactly like JS `||`).
#[derive(Debug, Clone, Copy, Default)]
pub struct FlexibleConstraints {
    pub max_edge: Option<u32>,
    pub max_pixels: Option<u64>,
    pub min_pixels: Option<u64>,
    pub max_ratio: Option<u64>,
}

impl FlexibleConstraints {
    fn max_edge(self) -> u32 {
        self.max_edge.filter(|v| *v != 0).unwrap_or(OPENAI_MAX_EDGE)
    }
    fn max_pixels(self) -> u64 {
        self.max_pixels
            .filter(|v| *v != 0)
            .unwrap_or(OPENAI_MAX_PIXELS)
    }
    fn min_pixels(self) -> u64 {
        self.min_pixels
            .filter(|v| *v != 0)
            .unwrap_or(OPENAI_MIN_PIXELS)
    }
    fn max_ratio(self) -> u64 {
        self.max_ratio
            .filter(|v| *v != 0)
            .unwrap_or(OPENAI_MAX_RATIO)
    }
}

/// The caller-side inputs of `resolveOpenAiImageSize`: the collapsed
/// request fields (the service layer folds the incumbent's
/// aspect_ratio/aspectRatio/ratio aliases into `ratio`) plus the raw
/// provider-defaults object (the dialect reads the same keys the incumbent
/// reads, in the same order).
#[derive(Debug, Clone, Copy)]
pub struct OpenAiSizeInput<'a> {
    pub size: Option<&'a str>,
    pub resolution: Option<&'a str>,
    pub ratio: Option<&'a str>,
    pub provider_defaults: &'a serde_json::Value,
    pub options: OpenAiSizeOptions<'a>,
}

fn error_prefix(source_name: &str) -> String {
    if source_name.is_empty() {
        String::new()
    } else {
        format!("{source_name} ")
    }
}

/// The JS-truthiness of a provider-defaults value as a string source:
/// `""`/`0`/`false`/`null` are skipped, any other value renders like
/// `String(value)`.
pub fn truthy_json_string(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
        serde_json::Value::Number(n) => {
            let rendered = n.to_string();
            if rendered == "0" || rendered == "0.0" || rendered == "-0.0" {
                None
            } else {
                Some(rendered)
            }
        }
        serde_json::Value::Bool(true) => Some("true".to_string()),
        _ => None,
    }
}

/// `parseRatio`: `^(\d+)\s*:\s*(\d+)$` on the trimmed string, both sides
/// positive; the label re-renders the parsed integers (`"04:3"` → `"4:3"`).
fn parse_ratio(value: &str) -> Option<(String, f64)> {
    let trimmed = value.trim();
    let (left, right) = trimmed.split_once(':')?;
    let left = left.trim();
    let right = right.trim();
    if left.is_empty()
        || right.is_empty()
        || !left.bytes().all(|b| b.is_ascii_digit())
        || !right.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let width: u64 = left.parse().ok()?;
    let height: u64 = right.parse().ok()?;
    if width == 0 || height == 0 {
        return None;
    }
    Some((format!("{width}:{height}"), width as f64 / height as f64))
}

/// `parsePixelSize`: `^(\d{2,5})\s*[x*]\s*(\d{2,5})$` (x case-insensitive)
/// on the trimmed string. Returns `(width, height, "WxH")`.
fn parse_pixel_size(value: &str) -> Option<(u32, u32, String)> {
    let trimmed = value.trim();
    let bytes = trimmed.as_bytes();
    let mut index = 0usize;
    let take_digits = |bytes: &[u8], index: &mut usize| -> Option<u32> {
        let start = *index;
        while *index < bytes.len() && bytes[*index].is_ascii_digit() {
            *index += 1;
        }
        let digits = &bytes[start..*index];
        if !(2..=5).contains(&digits.len()) {
            return None;
        }
        std::str::from_utf8(digits).ok()?.parse().ok()
    };
    let skip_spaces = |bytes: &[u8], index: &mut usize| {
        while *index < bytes.len() && (bytes[*index] as char).is_whitespace() {
            *index += 1;
        }
    };
    let width = take_digits(bytes, &mut index)?;
    skip_spaces(bytes, &mut index);
    if index >= bytes.len() || !matches!(bytes[index], b'x' | b'X' | b'*') {
        return None;
    }
    index += 1;
    skip_spaces(bytes, &mut index);
    let height = take_digits(bytes, &mut index)?;
    if index != bytes.len() {
        return None;
    }
    if width == 0 || height == 0 {
        return None;
    }
    Some((width, height, format!("{width}x{height}")))
}

/// `normalizeResolutionTier`: `None`/empty/whitespace → `None`; `auto`
/// passes through; `^([124])\s*k$` (lowercased) → `{d}k`; anything else is
/// the loud `image {source} "{raw}" is unsupported` (note: the literal
/// `image ` prefix, NOT the caller's sourceName — incumbent quirk, ported).
pub fn normalize_resolution_tier(
    value: Option<&str>,
    source: &str,
) -> Result<Option<String>, ProtocolError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    let raw = value.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    let normalized = raw.to_lowercase();
    if normalized == "auto" {
        return Ok(Some("auto".to_string()));
    }
    let bytes = normalized.as_bytes();
    let mut index = 0usize;
    if index < bytes.len() && matches!(bytes[index], b'1' | b'2' | b'4') {
        let digit = bytes[index] as char;
        index += 1;
        while index < bytes.len() && (bytes[index] as char).is_whitespace() {
            index += 1;
        }
        if index == bytes.len() - 1 && bytes[index] == b'k' {
            return Ok(Some(format!("{digit}k")));
        }
    }
    Err(parse::invalid(format!(
        "image {source} \"{raw}\" is unsupported"
    )))
}

/// `normalizedSupportedResolutions`: `None` for an absent/empty list; each
/// entry normalized (a garbage entry throws, exactly like the incumbent).
fn normalized_supported_resolutions(
    values: Option<&[&str]>,
) -> Result<Option<Vec<String>>, ProtocolError> {
    let Some(values) = values else {
        return Ok(None);
    };
    if values.is_empty() {
        return Ok(None);
    }
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        if let Some(tier) = normalize_resolution_tier(Some(value), "resolution")? {
            out.push(tier);
        }
    }
    Ok(Some(out))
}

/// `assertSupportedResolutionTier`: the refusal lists the RAW supported
/// values (incumbent: `supportedResolutions.join(", ")`).
fn assert_supported_resolution_tier(
    tier: &str,
    raw_value: &str,
    supported_resolutions: Option<&[&str]>,
    source_name: &str,
) -> Result<(), ProtocolError> {
    let Some(supported) = normalized_supported_resolutions(supported_resolutions)? else {
        return Ok(());
    };
    if supported.iter().any(|entry| entry == tier) {
        return Ok(());
    }
    let raw_list = supported_resolutions.unwrap_or_default().join(", ");
    Err(parse::invalid(format!(
        "{}resolution \"{raw_value}\" is unsupported; supported resolutions: {raw_list}",
        error_prefix(source_name)
    )))
}

/// `normalizeRatio`: absent/empty → `None`; an unparseable or unlisted
/// ratio is the loud `{source} ratio "{value}" is unsupported` (the RAW
/// value, not the trimmed one).
pub fn normalize_ratio(
    value: Option<&str>,
    supported_ratios: &[&str],
    source_name: &str,
) -> Result<Option<String>, ProtocolError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    let parsed = parse_ratio(value);
    let Some((label, _)) = parsed else {
        return Err(parse::invalid(format!(
            "{}ratio \"{value}\" is unsupported",
            error_prefix(source_name)
        )));
    };
    if !supported_ratios.contains(&label.as_str()) {
        return Err(parse::invalid(format!(
            "{}ratio \"{value}\" is unsupported",
            error_prefix(source_name)
        )));
    }
    Ok(Some(label))
}

/// `validateOpenAiFlexiblePixelSize` (verbatim checks and texts, in order).
fn validate_flexible_pixel_size(
    width: u32,
    height: u32,
    size: &str,
    source_name: &str,
    constraints: FlexibleConstraints,
) -> Result<String, ProtocolError> {
    let prefix = error_prefix(source_name);
    if !width.is_multiple_of(16) || !height.is_multiple_of(16) {
        return Err(parse::invalid(format!(
            "{prefix}size \"{size}\" is unsupported: width and height must be multiples of 16"
        )));
    }
    let max_edge = constraints.max_edge();
    if width.max(height) > max_edge {
        return Err(parse::invalid(format!(
            "{prefix}size \"{size}\" is unsupported: maximum edge is {max_edge}px"
        )));
    }
    let edge_ratio = width.max(height) as f64 / width.min(height) as f64;
    let max_ratio = constraints.max_ratio();
    if edge_ratio > max_ratio as f64 {
        return Err(parse::invalid(format!(
            "{prefix}size \"{size}\" is unsupported: aspect ratio exceeds {max_ratio}:1"
        )));
    }
    let pixels = width as u64 * height as u64;
    let min_pixels = constraints.min_pixels();
    let max_pixels = constraints.max_pixels();
    if pixels < min_pixels || pixels > max_pixels {
        return Err(parse::invalid(format!(
            "{prefix}size \"{size}\" is unsupported: total pixels must be between \
             {min_pixels} and {max_pixels}"
        )));
    }
    Ok(size.to_string())
}

/// `nearestOpenAiStandardSize`: argmin |ln(actual/ratio)| over the three
/// standard sizes; a tie keeps the larger-pixel candidate (and the FIRST
/// best on a full tie — the incumbent's strict `<` comparisons).
pub fn nearest_openai_standard_size(
    ratio_label: Option<&str>,
    source_name: &str,
) -> Result<String, ProtocolError> {
    let label = ratio_label.unwrap_or("1:1");
    let Some((_, ratio_value)) = parse_ratio(label) else {
        return Err(parse::invalid(format!(
            "{}ratio \"{label}\" is unsupported",
            error_prefix(source_name)
        )));
    };
    let mut best: Option<(&str, u64, f64)> = None;
    for size in STANDARD_SIZES {
        let (width, height, _) = parse_pixel_size(size).expect("the standard table parses");
        let actual_ratio = width as f64 / height as f64;
        let ratio_error = (actual_ratio / ratio_value).ln().abs();
        let pixels = width as u64 * height as u64;
        let better = match best {
            None => true,
            Some((_, best_pixels, best_error)) => {
                ratio_error < best_error || (ratio_error == best_error && pixels > best_pixels)
            }
        };
        if better {
            best = Some((size, pixels, ratio_error));
        }
    }
    Ok(best.expect("the standard table is non-empty").0.to_string())
}

/// `nearestOpenAiFlexibleSize`: the width-stepped search (16..=maxEdge step
/// 16; height candidates rounded-16 ±16), candidate ordering by
/// longEdgeError (4K: undershoot-only) → ratioError → pixelScore (4K:
/// larger pixels win), first-best on full ties.
pub fn nearest_openai_flexible_size(
    tier: &str,
    ratio_label: Option<&str>,
    source_name: &str,
    constraints: FlexibleConstraints,
) -> Result<String, ProtocolError> {
    let normalized_tier =
        normalize_resolution_tier(Some(tier), "resolution")?.unwrap_or_else(|| "1k".to_string());
    if normalized_tier == "auto" {
        return Ok("auto".to_string());
    }
    let ratio_display = ratio_label.unwrap_or("1:1");
    let Some((_, ratio_value)) = parse_ratio(ratio_display) else {
        return Err(parse::invalid(format!(
            "{}ratio \"{ratio_display}\" is unsupported",
            error_prefix(source_name)
        )));
    };
    let target_long_edge = TIER_LONG_EDGE
        .iter()
        .find(|(name, _)| *name == normalized_tier)
        .map(|(_, edge)| *edge);
    let Some(target_long_edge) = target_long_edge else {
        return Err(parse::invalid(format!(
            "{}resolution \"{tier}\" is unsupported",
            error_prefix(source_name)
        )));
    };
    let max_edge = constraints.max_edge();
    let max_pixels = constraints.max_pixels();
    let min_pixels = constraints.min_pixels();
    let max_ratio = constraints.max_ratio();

    #[derive(Clone, Copy)]
    struct Candidate {
        width: u32,
        height: u32,
        ratio_error: f64,
        long_edge_error: f64,
        pixel_score: f64,
    }

    let mut best: Option<Candidate> = None;
    let mut width = 16u32;
    while width <= max_edge {
        let ideal_height = width as f64 / ratio_value;
        let rounded_height = ((ideal_height / 16.0).round() as i64 * 16).max(16);
        for height in [rounded_height - 16, rounded_height, rounded_height + 16] {
            if height < 16 || height > max_edge as i64 || height % 16 != 0 {
                continue;
            }
            let height = height as u32;
            let edge_ratio = width.max(height) as f64 / width.min(height) as f64;
            if edge_ratio > max_ratio as f64 {
                continue;
            }
            let pixels = width as u64 * height as u64;
            if pixels < min_pixels || pixels > max_pixels {
                continue;
            }
            let long_edge = width.max(height) as f64;
            let actual_ratio = width as f64 / height as f64;
            let ratio_error = (actual_ratio / ratio_value).ln().abs();
            let long_edge_error = if normalized_tier == "4k" {
                (target_long_edge as f64 - long_edge).max(0.0)
            } else {
                (long_edge - target_long_edge as f64).abs()
            };
            let pixel_score = if normalized_tier == "4k" {
                -(pixels as f64)
            } else {
                (pixels as f64 - (target_long_edge as f64).powi(2)).abs()
            };
            let candidate = Candidate {
                width,
                height,
                ratio_error,
                long_edge_error,
                pixel_score,
            };
            let better = match best {
                None => true,
                Some(b) => {
                    candidate.long_edge_error < b.long_edge_error
                        || (candidate.long_edge_error == b.long_edge_error
                            && candidate.ratio_error < b.ratio_error)
                        || (candidate.long_edge_error == b.long_edge_error
                            && candidate.ratio_error == b.ratio_error
                            && candidate.pixel_score < b.pixel_score)
                }
            };
            if better {
                best = Some(candidate);
            }
        }
        width += 16;
    }
    let Some(best) = best else {
        return Err(parse::invalid(format!(
            "{}could not resolve {normalized_tier} {ratio_display} to a supported size",
            error_prefix(source_name)
        )));
    };
    Ok(format!("{}x{}", best.width, best.height))
}

/// `normalizeOpenAiSizeInput`: pixel sizes validate against the flexible or
/// standard vocabulary; tier strings go through the supported-tier assert
/// and resolve to a nearest size (flexible) or the nearest standard size
/// (non-flexible, tier ignored after the assert — incumbent behavior).
#[allow(clippy::too_many_arguments)]
fn normalize_openai_size_input(
    value: Option<&str>,
    ratio: Option<&str>,
    flexible: bool,
    source_name: &str,
    supported_resolutions: Option<&[&str]>,
    constraints: FlexibleConstraints,
) -> Result<Option<String>, ProtocolError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    let raw = value.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    if raw.to_lowercase() == "auto" {
        return Ok(Some("auto".to_string()));
    }
    if let Some((width, height, size)) = parse_pixel_size(raw) {
        if flexible {
            return validate_flexible_pixel_size(width, height, &size, source_name, constraints)
                .map(Some);
        }
        if STANDARD_SIZES_BY_RATIO
            .iter()
            .any(|(_, standard)| *standard == size)
        {
            return Ok(Some(size));
        }
        return Err(parse::invalid(format!(
            "{}size \"{raw}\" is unsupported",
            error_prefix(source_name)
        )));
    }
    if let Some(tier) = normalize_resolution_tier(Some(raw), "size")? {
        assert_supported_resolution_tier(&tier, raw, supported_resolutions, source_name)?;
        return if flexible {
            nearest_openai_flexible_size(&tier, ratio.or(Some("1:1")), source_name, constraints)
                .map(Some)
        } else {
            nearest_openai_standard_size(ratio.or(Some("1:1")), source_name).map(Some)
        };
    }
    Err(parse::invalid(format!(
        "{}size \"{raw}\" is unsupported",
        error_prefix(source_name)
    )))
}

/// `resolveOpenAiImageSize` (the incumbent priority chain, verbatim):
/// params.size → params.resolution → ratio (+ provider default resolution
/// or the option default) → providerDefaults.size → providerDefaults
/// .resolution → `None` (the field is then omitted from the wire body).
pub fn resolve_openai_image_size(input: &OpenAiSizeInput) -> Result<Option<String>, ProtocolError> {
    let options = &input.options;
    let source_name = options.source_name;
    let flexible = options.flexible;
    let default_ratios: &[&str] = if flexible {
        &OPENAI_FLEXIBLE_IMAGE_RATIOS
    } else {
        &OPENAI_STANDARD_IMAGE_RATIOS
    };
    let supported_ratios = options.supported_ratios.unwrap_or(default_ratios);
    let default_tiers: &[&str] = if flexible {
        &OPENAI_FLEXIBLE_RESOLUTION_TIERS
    } else {
        &OPENAI_STANDARD_RESOLUTION_TIERS
    };
    let supported_resolutions = options.supported_resolutions.or(Some(default_tiers));
    let constraints = options.constraints;
    let defaults = input.provider_defaults;

    // The incumbent ratio chain reads the request aliases first (collapsed
    // into `input.ratio` by the caller), then the provider defaults in
    // declaration order, then the option default.
    let effective_ratio = input
        .ratio
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .or_else(|| truthy_json_string(defaults.get("aspect_ratio")))
        .or_else(|| truthy_json_string(defaults.get("aspectRatio")))
        .or_else(|| truthy_json_string(defaults.get("ratio")))
        .or_else(|| options.default_ratio.map(str::to_string));
    let ratio = normalize_ratio(effective_ratio.as_deref(), supported_ratios, source_name)?;

    if let Some(size) = input.size.filter(|v| !v.is_empty()) {
        return normalize_openai_size_input(
            Some(size),
            ratio.as_deref(),
            flexible,
            source_name,
            supported_resolutions,
            constraints,
        );
    }
    if let Some(resolution) = input.resolution.filter(|v| !v.is_empty()) {
        return normalize_openai_size_input(
            Some(resolution),
            ratio.as_deref().or(Some("1:1")),
            flexible,
            source_name,
            supported_resolutions,
            constraints,
        );
    }
    if let Some(ratio) = ratio.as_deref() {
        let default_resolution = truthy_json_string(defaults.get("resolution"))
            .or_else(|| options.default_resolution.map(str::to_string));
        if let Some(default_resolution) = default_resolution {
            return normalize_openai_size_input(
                Some(&default_resolution),
                Some(ratio),
                flexible,
                source_name,
                supported_resolutions,
                constraints,
            );
        }
        return nearest_openai_standard_size(Some(ratio), source_name).map(Some);
    }
    if let Some(size) = truthy_json_string(defaults.get("size")) {
        return normalize_openai_size_input(
            Some(&size),
            ratio.as_deref(),
            flexible,
            source_name,
            supported_resolutions,
            constraints,
        );
    }
    if let Some(resolution) = truthy_json_string(defaults.get("resolution")) {
        return normalize_openai_size_input(
            Some(&resolution),
            options.default_ratio.or(Some("1:1")),
            flexible,
            source_name,
            supported_resolutions,
            constraints,
        );
    }
    Ok(None)
}
