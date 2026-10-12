//! R06-T07 知识库纯逻辑：错误码、token 估算、chunker、跨库身份算法、RRF、
//! scope 规则与维度门策略。
//!
//! 现役锚点（RC-2，base 工作区亲验）：
//! - `lib/knowledge/errors.ts`（错误码表 + 稳定脱敏消息纪律）
//! - `lib/llm/estimate-text-tokens.ts:13-46`（CJK 1.1 tok/char、其余 4 char/tok）
//! - `lib/knowledge/chunker.ts`（v4 chunker：configId/sections/chunks/spans/指纹）
//! - `lib/knowledge/vector-index-adapter.ts:84-104`（civ_/viv_ 身份算法）
//! - `lib/knowledge/knowledge-query-service.ts:173-206`（modelKey）、:238（RRF k=60）
//! - `lib/tools/knowledge-scope.ts`（scope 复核链）
//!
//! 退役边界：不携带任何独立知识研究引擎（research/analysis/coverage）的领域
//! 规则——那些表与流程属兼容残留，不迁移（F-D08-C-RESEARCH-LEDGER）。

use sha2::{Digest, Sha256};

/// 知识域错误（errors.ts 的稳定错误码 + 脱敏消息；details 不携带原始路径/正文）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeError {
    pub code: String,
    pub message: String,
}

impl KnowledgeError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new("KNOWLEDGE_INVALID_ARGUMENT", message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("KNOWLEDGE_NOT_FOUND", message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new("KNOWLEDGE_CONFLICT", message)
    }

    pub fn scope_violation(message: impl Into<String>) -> Self {
        Self::new("KNOWLEDGE_SCOPE_VIOLATION", message)
    }

    pub fn retrieval_unavailable(message: impl Into<String>) -> Self {
        Self::new("KNOWLEDGE_RETRIEVAL_UNAVAILABLE", message)
    }

    pub fn index_invalid(message: impl Into<String>) -> Self {
        Self::new("KNOWLEDGE_INDEX_INVALID", message)
    }

    pub fn storage_invalid(message: impl Into<String>) -> Self {
        Self::new("KNOWLEDGE_STORAGE_INVALID", message)
    }
}

impl std::fmt::Display for KnowledgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for KnowledgeError {}

/// TS `errorStatus`（routes/knowledge.ts:30-62）的错误码 → HTTP 状态映射。
pub fn knowledge_error_status(code: &str) -> u16 {
    match code {
        "KNOWLEDGE_NOT_FOUND" => 404,
        "KNOWLEDGE_CONFLICT" => 409,
        "KNOWLEDGE_IMPORT_PATH_BLOCKED" | "KNOWLEDGE_WEB_URL_BLOCKED" => 403,
        "KNOWLEDGE_IMPORT_TOO_LARGE" | "KNOWLEDGE_WEB_TOO_LARGE" => 413,
        "KNOWLEDGE_PARSE_FAILED"
        | "KNOWLEDGE_PARSE_NOT_READY"
        | "KNOWLEDGE_IMPORT_TYPE_UNSUPPORTED"
        | "KNOWLEDGE_SCOPE_NOT_READY"
        | "KNOWLEDGE_RETRIEVAL_EMPTY"
        | "KNOWLEDGE_WEB_TYPE_UNSUPPORTED" => 422,
        "KNOWLEDGE_MODEL_OUTPUT_INVALID" | "KNOWLEDGE_WEB_FETCH_FAILED" => 502,
        "KNOWLEDGE_MODEL_UNAVAILABLE"
        | "KNOWLEDGE_RETRIEVAL_UNAVAILABLE"
        | "KNOWLEDGE_INDEX_INVALID" => 503,
        "KNOWLEDGE_STORAGE_INVALID" | "KNOWLEDGE_SCHEMA_NEWER" => 500,
        _ => 400,
    }
}

// ── 哈希与身份算法（跨库身份键，逐字节冻结）────────────────────────────────

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(bytes);
    let digest = hash.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// ChunkIndexVariant 确定性 id：`civ_` + sha256(parseArtifactId + '\0' + hash)[0..32]
/// （vector-index-adapter.ts:91-96）。
pub fn chunk_index_variant_id(parse_artifact_id: &str, chunk_profile_hash: &str) -> String {
    format!(
        "civ_{}",
        &sha256_hex(format!("{parse_artifact_id}\0{chunk_profile_hash}").as_bytes())[..32]
    )
}

/// VectorIndexVariant 确定性 id：`viv_` + sha256(civ + '\0' + modelKey)[0..32]
/// （vector-index-adapter.ts:99-104）。
pub fn vector_index_variant_id(chunk_index_variant_id: &str, model_key: &str) -> String {
    format!(
        "viv_{}",
        &sha256_hex(format!("{chunk_index_variant_id}\0{model_key}").as_bytes())[..32]
    )
}

/// 嵌入模型身份 key：sha256(JSON[provider, modelId, protocol, dimensions])
/// （knowledge-query-service.ts:173-206 的 descriptor 逐字节口径）。
pub fn embedding_model_key(
    provider: &str,
    model_id: &str,
    protocol: &str,
    dimensions: u32,
) -> String {
    let descriptor = serde_json::to_string(&(provider, model_id, protocol, dimensions))
        .unwrap_or_else(|_| format!("[{provider},{model_id},{protocol},{dimensions}]"));
    sha256_hex(descriptor.as_bytes())
}

/// 一次已解析嵌入身份（查询与写入两侧共用；写入侧落 vector_index_variants）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingModelIdentity {
    pub key: String,
    pub provider: String,
    pub model_id: String,
    pub protocol: String,
    pub dimensions: u32,
    /// 未发送的配置身份也可提供该指纹，但 dimensions=0 明确表示维度尚未实测。
    pub parameter_fingerprint: String,
}

impl EmbeddingModelIdentity {
    pub fn new(provider: &str, model_id: &str, protocol: &str, dimensions: u32) -> Self {
        Self {
            key: embedding_model_key(provider, model_id, protocol, dimensions),
            provider: provider.to_string(),
            model_id: model_id.to_string(),
            protocol: protocol.to_string(),
            dimensions,
            parameter_fingerprint: String::new(),
        }
    }

    /// 新生产身份含端点及语义参数，旧 key 不自动冒认兼容，保留旧变体待重建。
    pub fn with_parameters(
        provider: &str,
        model_id: &str,
        protocol: &str,
        dimensions: u32,
        parameter_fingerprint: String,
    ) -> Self {
        let mut identity = Self::new(provider, model_id, protocol, dimensions);
        let descriptor = serde_json::json!([
            "lingxi.embedding-identity.v2",
            provider,
            model_id,
            protocol,
            dimensions,
            parameter_fingerprint
        ]);
        identity.key = sha256_hex(descriptor.to_string().as_bytes());
        identity.parameter_fingerprint = parameter_fingerprint;
        identity
    }
}

// ── token 估算（lib/llm/estimate-text-tokens.ts:13-46）─────────────────────

pub const CJK_TOKENS_PER_CHAR: f64 = 1.1;
pub const NON_CJK_CHARS_PER_TOKEN: f64 = 4.0;

fn is_cjk_code_point(code_point: char) -> bool {
    matches!(code_point,
        '\u{1100}'..='\u{11ff}'
        | '\u{2e80}'..='\u{9fff}'
        | '\u{ac00}'..='\u{d7af}'
        | '\u{f900}'..='\u{faff}'
        | '\u{fe30}'..='\u{fe4f}'
        | '\u{ff00}'..='\u{ffef}'
        | '\u{20000}'..='\u{2fa1f}')
}

/// 单字符串 token 估算：CJK 按字符 ×1.1，其余 4 char/token，向上取整。
pub fn estimate_text_tokens(text: &str) -> usize {
    let mut cjk: usize = 0;
    let mut total: usize = 0;
    for ch in text.chars() {
        total += 1;
        if is_cjk_code_point(ch) {
            cjk += 1;
        }
    }
    ((cjk as f64) * CJK_TOKENS_PER_CHAR + ((total - cjk) as f64) / NON_CJK_CHARS_PER_TOKEN).ceil()
        as usize
}

// ── chunker（v4；chunker.ts 冻结算法）──────────────────────────────────────

pub const KNOWLEDGE_CHUNKER_VERSION: &str = "4";
pub const KNOWLEDGE_SECTION_VERSION: &str = "3";
pub const KNOWLEDGE_CHUNK_TARGET_CHARS: usize = 2048;
pub const MIN_KNOWLEDGE_CHUNK_TARGET_CHARS: usize = 100;
pub const MAX_KNOWLEDGE_CHUNK_TARGET_CHARS: usize = 100_000;
pub const KNOWLEDGE_CHUNK_CONTEXT_FRACTION: f64 = 0.8;
pub const KNOWLEDGE_CHUNK_FALLBACK_CONTEXT_TOKENS: f64 = 8192.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkerStrategy {
    Fixed,
    Markdown,
    Text,
    Pdf,
    Html,
}

impl ChunkerStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            ChunkerStrategy::Fixed => "fixed",
            ChunkerStrategy::Markdown => "markdown",
            ChunkerStrategy::Text => "text",
            ChunkerStrategy::Pdf => "pdf",
            ChunkerStrategy::Html => "html",
        }
    }
}

/// chunker 配置指纹：sha256(`4{strategy}{targetChars}`)[0..16]
/// （chunker.ts:120-129 逐字节口径）。
pub fn chunker_config_id(strategy: ChunkerStrategy, target_chars: usize) -> String {
    sha256_hex(
        format!(
            "{}{}{}",
            KNOWLEDGE_CHUNKER_VERSION,
            strategy.as_str(),
            target_chars
        )
        .as_bytes(),
    )[..16]
        .to_string()
}

/// 自动分块尺寸 = 嵌入窗口 × 80%（1 token = 1 字符最保守口径），夹在 MIN/MAX；
/// 窗口未知回退 8192（chunker.ts:100-110）。
pub fn compute_auto_chunk_target_chars(context_window_tokens: Option<f64>) -> usize {
    let window = match context_window_tokens {
        Some(value) if value.is_finite() && value > 0.0 => value,
        _ => KNOWLEDGE_CHUNK_FALLBACK_CONTEXT_TOKENS,
    };
    let target = (window * KNOWLEDGE_CHUNK_CONTEXT_FRACTION).floor() as i64;
    (target as usize).clamp(
        MIN_KNOWLEDGE_CHUNK_TARGET_CHARS,
        MAX_KNOWLEDGE_CHUNK_TARGET_CHARS,
    )
}

fn normalize_target_chars(target_chars: Option<usize>) -> Result<usize, KnowledgeError> {
    let value = target_chars.unwrap_or(KNOWLEDGE_CHUNK_TARGET_CHARS);
    if !(MIN_KNOWLEDGE_CHUNK_TARGET_CHARS..=MAX_KNOWLEDGE_CHUNK_TARGET_CHARS).contains(&value) {
        return Err(KnowledgeError::new(
            "KNOWLEDGE_INVALID_ARGUMENT",
            format!("Knowledge chunk targetChars is invalid: {value}"),
        ));
    }
    Ok(value)
}

/// 解析块（服务侧产出的解析产物行）。
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkerBlock {
    pub id: String,
    pub ordinal: usize,
    pub text: String,
    pub text_sha256: String,
    pub locator_type: String,
    /// 解析器 locator（headingPath / lineStart / page 等）。
    pub locator: serde_json::Value,
}

impl ChunkerBlock {
    fn heading_path(&self) -> Vec<String> {
        locator_heading_path(&self.locator)
    }
}

/// locator.headingPath 的有效标题（稀疏 null 形态过滤，chunker.ts:149-154 口径）。
pub fn locator_heading_path(locator: &serde_json::Value) -> Vec<String> {
    locator
        .get("headingPath")
        .and_then(|value| value.as_array())
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_str())
                .filter(|entry| !entry.trim().is_empty())
                .map(|entry| entry.to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// 按首块 locatorType 分发策略；纯文本无章节结构回退 fixed（chunker.ts:160-184）。
pub fn resolve_chunker_config(
    blocks: &[ChunkerBlock],
    target_chars: Option<usize>,
) -> Result<(ChunkerStrategy, usize, String), KnowledgeError> {
    let target_chars = normalize_target_chars(target_chars)?;
    let strategy = match blocks.first().map(|block| block.locator_type.as_str()) {
        Some("markdown") => ChunkerStrategy::Markdown,
        Some("html") => ChunkerStrategy::Html,
        Some("pdf") => ChunkerStrategy::Pdf,
        Some("text") => {
            if blocks
                .iter()
                .any(|block| is_text_chapter_heading(&block.text))
            {
                ChunkerStrategy::Text
            } else {
                ChunkerStrategy::Fixed
            }
        }
        _ => ChunkerStrategy::Fixed,
    };
    let config_id = chunker_config_id(strategy, target_chars);
    Ok((strategy, target_chars, config_id))
}

/// 章节标题行首启发式（chunker.ts:132-143）。
pub fn is_text_chapter_heading(text: &str) -> bool {
    let trimmed = text.trim_start();
    let chars: Vec<char> = trimmed.chars().collect();
    // 第[零〇一二三四五六七八九十百千万\d]{1,7}[章节回卷部篇]
    if trimmed.starts_with('第') && chars.len() > 2 {
        let digit_count = chars[1..]
            .iter()
            .take_while(|ch| ch.is_ascii_digit() || "零〇一二三四五六七八九十百千万".contains(**ch))
            .count();
        if (1..=7).contains(&digit_count)
            && 1 + digit_count < chars.len()
            && "章节回卷部篇".contains(chars[1 + digit_count])
        {
            return true;
        }
    }
    if let Some(rest) = trimmed.strip_prefix("Chapter ") {
        if rest.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
            return true;
        }
    }
    ["序章", "楔子", "尾声", "终章", "番外", "后记"]
        .iter()
        .any(|marker| trimmed.starts_with(marker))
}

/// block 指纹：sha256(version + Σ('\0' + id + '\0' + textSha256))
/// （chunker.ts:657-670 逐字节口径）。
pub fn block_fingerprint(blocks: &[ChunkerBlock]) -> String {
    let mut hash = Sha256::new();
    hash.update(KNOWLEDGE_CHUNKER_VERSION.as_bytes());
    for block in blocks {
        hash.update(b"\0");
        hash.update(block.id.as_bytes());
        hash.update(b"\0");
        hash.update(block.text_sha256.as_bytes());
    }
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn deterministic_chunk_id(config_id: &str, parse_artifact_id: &str, ordinal: usize) -> String {
    format!(
        "chunk_{}",
        &sha256_hex(format!("{config_id}\0{parse_artifact_id}\0{ordinal}").as_bytes())[..32]
    )
}

fn section_id(parse_artifact_id: &str, section_ordinal: usize) -> String {
    let serialized = serde_json::to_string(&(
        KNOWLEDGE_SECTION_VERSION,
        parse_artifact_id,
        section_ordinal as u64,
    ))
    .unwrap_or_default();
    format!("section_{}", &sha256_hex(serialized.as_bytes())[..32])
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChunkSpan {
    pub block_id: String,
    pub block_start_offset: usize,
    pub block_end_offset: usize,
    pub chunk_start_offset: usize,
    pub chunk_end_offset: usize,
}

/// 分块产物（chunker.ts KnowledgeChunkDraft）。
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkDraft {
    pub id: String,
    pub ordinal: usize,
    pub section_id: Option<String>,
    pub text: String,
    pub token_count: usize,
    pub spans: Vec<ChunkSpan>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SectionDraft {
    pub id: String,
    pub section_ordinal: usize,
    pub heading_path: Vec<String>,
    pub start_block_ordinal: usize,
    pub end_block_ordinal: usize,
    pub text: String,
    pub token_count: usize,
    /// (block id, block 内起偏移, block 内止偏移, section 起偏移, section 止偏移)
    /// ——与 TS section span 双坐标系（blockStartOffset/sectionStartOffset）同构。
    pub block_spans: Vec<(String, usize, usize, usize, usize)>,
    pub primary_block_ids: Vec<String>,
}

/// 字符预算边界（按 char 推进；TS 为 UTF-16 code unit，Rust 侧以 Unicode scalar
/// 为等价锚——差异台账 D 项申报）。
fn character_boundary(text: &[char], start: usize, limit: usize) -> usize {
    (start + limit).min(text.len())
}

/// 在目标片段后四成内依次找段落、换行、句末和词间边界（chunker.ts:521-537）。
fn preferred_chunk_end(text: &[char], start: usize, end: usize) -> usize {
    if end >= text.len() {
        return text.len();
    }
    let floor = start + (end - start) * 3 / 5;
    let mut last_boundary: Option<usize>;
    for class in 0..4 {
        last_boundary = None;
        for cursor in floor..end {
            let ch = text[cursor];
            let boundary = match class {
                0 => ch == '\n' && cursor + 1 < end && text[cursor + 1] == '\n',
                1 => ch == '\n' || ch == '\r',
                2 => matches!(ch, '。' | '！' | '？' | '；' | '.' | '!' | '?' | ';'),
                _ => ch.is_whitespace(),
            };
            if boundary {
                last_boundary = Some(cursor + 1);
            }
        }
        if let Some(found) = last_boundary {
            if found > floor {
                return found.min(end);
            }
        }
    }
    end
}

/// 章节是原文的确定性连续分区（chunker.ts:541-614）。
pub fn build_sections(parse_artifact_id: &str, blocks: &[ChunkerBlock]) -> Vec<SectionDraft> {
    let soft_limit = 8192_usize;
    let mut sections: Vec<SectionDraft> = Vec::new();
    let mut owned_blocks: std::collections::HashSet<String> = std::collections::HashSet::new();

    // primarySectionGroups：按 heading/chapter 分组。
    let mut groups: Vec<(Vec<String>, Vec<&ChunkerBlock>)> = Vec::new();
    let mut previous_key: Option<String> = None;
    let mut active_heading: Vec<String> = Vec::new();
    for block in blocks {
        let (heading_path, key) = match block.locator_type.as_str() {
            "markdown" | "html" => {
                let path = block.heading_path();
                let key = format!(
                    "heading:{}",
                    serde_json::to_string(&path).unwrap_or_default()
                );
                (path, key)
            }
            "text" => {
                let path = block.heading_path();
                if !path.is_empty() {
                    active_heading = path;
                } else if is_text_chapter_heading(&block.text) {
                    active_heading = vec![block.text.trim().to_string()];
                }
                let path = active_heading.clone();
                let key = format!(
                    "chapter:{}",
                    serde_json::to_string(&path).unwrap_or_default()
                );
                (path, key)
            }
            "pdf" => {
                let path = block.heading_path();
                if !path.is_empty() {
                    active_heading = path;
                }
                let path = active_heading.clone();
                let key = if path.is_empty() {
                    let page = block
                        .locator
                        .get("page")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    format!("page:{page}")
                } else {
                    format!(
                        "heading:{}",
                        serde_json::to_string(&path).unwrap_or_default()
                    )
                };
                (path, key)
            }
            _ => (Vec::new(), "plain".to_string()),
        };
        if previous_key.as_deref() != Some(key.as_str()) || groups.is_empty() {
            groups.push((heading_path.clone(), Vec::new()));
        }
        groups.last_mut().expect("group").1.push(block);
        previous_key = Some(key);
    }

    for (heading_path, group_blocks) in groups {
        let mut text: Vec<char> = Vec::new();
        // (block 索引, section-text 起点, section-text 终点)
        let mut positions: Vec<(usize, usize, usize)> = Vec::new();
        for (index, block) in group_blocks.iter().enumerate() {
            if !text.is_empty() {
                text.push('\n');
                text.push('\n');
            }
            let start = text.len();
            text.extend(block.text.chars());
            positions.push((index, start, text.len()));
        }
        let mut start = 0;
        loop {
            let end = character_boundary(&text, start, soft_limit).max(start);
            let mut block_spans: Vec<(String, usize, usize, usize, usize)> = Vec::new();
            let mut covered_ordinals: Vec<usize> = Vec::new();
            let mut primary_block_ids: Vec<String> = Vec::new();
            for (index, position_start, position_end) in &positions {
                let block = group_blocks[*index];
                let from = start.max(*position_start);
                let to = end.min(*position_end);
                if to <= from {
                    continue;
                }
                covered_ordinals.push(block.ordinal);
                if owned_blocks.insert(block.id.clone()) {
                    primary_block_ids.push(block.id.clone());
                }
                block_spans.push((
                    block.id.clone(),
                    from - position_start,
                    to - position_start,
                    from - start,
                    to - start,
                ));
            }
            let section_ordinal = sections.len();
            let section_text: String = text[start..end].iter().collect();
            let fallback_ordinal = positions
                .iter()
                .find(|(_, _, position_end)| *position_end >= start)
                .map(|(index, _, _)| group_blocks[*index].ordinal)
                .unwrap_or_else(|| group_blocks.last().expect("group").ordinal);
            sections.push(SectionDraft {
                id: section_id(parse_artifact_id, section_ordinal),
                section_ordinal,
                heading_path: heading_path.clone(),
                start_block_ordinal: covered_ordinals
                    .first()
                    .copied()
                    .unwrap_or(fallback_ordinal),
                end_block_ordinal: covered_ordinals.last().copied().unwrap_or(fallback_ordinal),
                token_count: estimate_text_tokens(&section_text),
                text: section_text,
                primary_block_ids,
                block_spans,
            });
            if end >= text.len() {
                break;
            }
            start = end;
        }
    }
    sections
}

/// 当前版分块：节内滑窗 + preferredChunkEnd + overlap（chunker.ts:617-646）。
pub fn build_chunks(
    parse_artifact_id: &str,
    blocks: &[ChunkerBlock],
    target_chars: Option<usize>,
) -> Result<Vec<ChunkDraft>, KnowledgeError> {
    let (_, target_chars, config_id) = resolve_chunker_config(blocks, target_chars)?;
    let overlap_chars = target_chars / 8;
    let mut chunks: Vec<ChunkDraft> = Vec::new();
    for section in build_sections(parse_artifact_id, blocks) {
        let section_chars: Vec<char> = section.text.chars().collect();
        let mut start = 0;
        while start < section_chars.len() {
            let raw_end = character_boundary(&section_chars, start, target_chars);
            let end = preferred_chunk_end(&section_chars, start, raw_end);
            let mut spans: Vec<ChunkSpan> = Vec::new();
            for (
                block_id,
                span_block_start,
                _span_block_end,
                span_section_start,
                span_section_end,
            ) in &section.block_spans
            {
                let from = start.max(*span_section_start);
                let to = end.min(*span_section_end);
                if to > from {
                    spans.push(ChunkSpan {
                        block_id: block_id.clone(),
                        block_start_offset: span_block_start + from - span_section_start,
                        block_end_offset: span_block_start + to - span_section_start,
                        chunk_start_offset: from - start,
                        chunk_end_offset: to - start,
                    });
                }
            }
            if !spans.is_empty() {
                let text: String = section_chars[start..end].iter().collect();
                let ordinal = chunks.len();
                chunks.push(ChunkDraft {
                    id: deterministic_chunk_id(&config_id, parse_artifact_id, ordinal),
                    ordinal,
                    section_id: Some(section.id.clone()),
                    token_count: estimate_text_tokens(&text),
                    text,
                    spans,
                });
            }
            if end >= section_chars.len() {
                break;
            }
            let next = character_boundary(&section_chars, end, overlap_chars);
            start = if next > start { next } else { end };
        }
    }
    Ok(chunks)
}

/// RRF 融合（k=60，knowledge-query-service.ts:238 fuseCandidates 口径）：
/// 输入为各通道的名次序（0-based），输出按融合分排序的候选 id。
pub fn fuse_rankings(channels: &[Vec<String>], limit: usize) -> Vec<(String, f64)> {
    const RRF_K: f64 = 60.0;
    let mut scores: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for channel in channels {
        for (rank, id) in channel.iter().enumerate() {
            *scores.entry(id.clone()).or_insert(0.0) += 1.0 / (RRF_K + rank as f64 + 1.0);
        }
    }
    let mut fused: Vec<(String, f64)> = scores.into_iter().collect();
    fused.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.0.cmp(&right.0))
    });
    fused.truncate(limit);
    fused
}

/// 余弦相似度（向量长度必须一致——不一致由调用方维度门拒绝）。
pub fn cosine(left: &[f32], right: &[f32]) -> Result<f32, KnowledgeError> {
    if left.is_empty() || left.len() != right.len() {
        return Err(KnowledgeError::retrieval_unavailable(
            "vector dimensions do not match the query identity",
        ));
    }
    let mut dot = 0.0_f64;
    let mut norm_left = 0.0_f64;
    let mut norm_right = 0.0_f64;
    for (l, r) in left.iter().zip(right.iter()) {
        dot += (*l as f64) * (*r as f64);
        norm_left += (*l as f64) * (*l as f64);
        norm_right += (*r as f64) * (*r as f64);
    }
    if norm_left == 0.0 || norm_right == 0.0 {
        return Ok(0.0);
    }
    Ok((dot / (norm_left.sqrt() * norm_right.sqrt())) as f32)
}

/// 归一化查询（query-embedding-cache.ts normalizeKnowledgeQuery 口径）。
pub fn normalize_knowledge_query(query: &str) -> String {
    query.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_goldens_match_the_frozen_algorithms() {
        // 现役同算法金样（TS：crypto.createHash("sha256")…）。
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let civ = chunk_index_variant_id("kpa_a", "hash1");
        assert!(civ.starts_with("civ_") && civ.len() == 36);
        assert_eq!(civ, chunk_index_variant_id("kpa_a", "hash1"));
        assert_ne!(civ, chunk_index_variant_id("kpa_a", "hash2"));
        let viv = vector_index_variant_id(&civ, "key1");
        assert!(viv.starts_with("viv_") && viv.len() == 36);
        assert_ne!(viv, vector_index_variant_id(&civ, "key2"));
        // modelKey descriptor = JSON 数组逐字节口径。
        let key = embedding_model_key("embed", "embed-model-a", "openai-embeddings", 2);
        let expected = sha256_hex(br#"["embed","embed-model-a","openai-embeddings",2]"#);
        assert_eq!(key, expected);
    }

    #[test]
    fn config_id_golden_and_auto_target() {
        // sha256("4markdown800")[0..16]。
        let expected = &sha256_hex(b"4markdown800")[..16].to_string();
        assert_eq!(chunker_config_id(ChunkerStrategy::Markdown, 800), *expected);
        assert_eq!(compute_auto_chunk_target_chars(Some(2000.0)), 1600);
        assert_eq!(
            compute_auto_chunk_target_chars(Some(5.0)),
            MIN_KNOWLEDGE_CHUNK_TARGET_CHARS
        );
        assert_eq!(compute_auto_chunk_target_chars(None), 6553); // floor(8192*0.8)
    }

    #[test]
    fn token_estimator_matches_the_incumbent_formula() {
        assert_eq!(estimate_text_tokens(""), 0);
        assert_eq!(estimate_text_tokens("abcd"), 1); // 4/4
        assert_eq!(estimate_text_tokens("ab"), 1); // ceil(2/4)
        assert_eq!(estimate_text_tokens("灵犀"), 3); // ceil(2*1.1)
        assert_eq!(estimate_text_tokens("灵犀abc"), 3); // ceil(2.2 + 3/4) = ceil(2.95)
    }

    #[test]
    fn chunker_produces_sections_and_locatable_spans() {
        let mk = |ordinal: usize, text: &str, heading: &str| ChunkerBlock {
            id: format!("kb_{ordinal}"),
            ordinal,
            text: text.to_string(),
            text_sha256: sha256_hex(text.as_bytes()),
            locator_type: "markdown".into(),
            locator: serde_json::json!({"headingPath": [heading], "lineStart": ordinal + 1, "lineEnd": ordinal + 1}),
        };
        let blocks = vec![
            mk(0, "# 第一章 架构", "第一章 架构"),
            mk(1, "灵犀是有记忆的个人 AI 助手。", "第一章 架构"),
            mk(2, "# 第二章 检索", "第二章 检索"),
            mk(3, "知识库支持来源作用域检索。", "第二章 检索"),
        ];
        let (strategy, target, config_id) =
            resolve_chunker_config(&blocks, Some(800)).expect("config");
        assert_eq!(strategy, ChunkerStrategy::Markdown);
        assert_eq!(target, 800);
        assert_eq!(config_id, chunker_config_id(ChunkerStrategy::Markdown, 800));
        let sections = build_sections("kpa_1", &blocks);
        assert!(
            sections.len() >= 2,
            "heading changes split sections: {sections:?}"
        );
        assert_eq!(sections[0].heading_path, vec!["第一章 架构".to_string()]);
        assert!(sections[0].id.starts_with("section_"));
        let chunks = build_chunks("kpa_1", &blocks, Some(800)).expect("chunks");
        assert!(!chunks.is_empty());
        for chunk in &chunks {
            assert!(chunk.id.starts_with("chunk_"));
            assert!(!chunk.spans.is_empty());
            for span in &chunk.spans {
                // 跨度可回原块：chunk 内文本切片 == 原块对应切片。
                let block = blocks
                    .iter()
                    .find(|b| b.id == span.block_id)
                    .expect("block");
                let block_chars: Vec<char> = block.text.chars().collect();
                assert!(
                    span.block_end_offset <= block_chars.len(),
                    "span inside block"
                );
                let chunk_chars: Vec<char> = chunk.text.chars().collect();
                assert!(span.chunk_end_offset <= chunk_chars.len());
                assert_eq!(
                    block_chars[span.block_start_offset..span.block_end_offset]
                        .iter()
                        .collect::<String>(),
                    chunk_chars[span.chunk_start_offset..span.chunk_end_offset]
                        .iter()
                        .collect::<String>(),
                    "chunk span must map back to the block text byte-for-byte"
                );
            }
        }
        // 分块配置指纹进入 chunk id 派生（不同配置不同 id）。
        let other = build_chunks("kpa_1", &blocks, Some(300)).expect("chunks 300");
        assert_ne!(chunks[0].id, other[0].id);
    }

    #[test]
    fn rrf_fusion_and_cosine() {
        let fused = fuse_rankings(
            &[vec!["a".into(), "b".into()], vec!["b".into(), "c".into()]],
            10,
        );
        // b 在两通道都出现 → 排第一。
        assert_eq!(fused[0].0, "b");
        let score = cosine(&[1.0, 0.0], &[1.0, 0.0]).expect("cos");
        assert!((score - 1.0).abs() < 1e-6);
        let err = cosine(&[1.0], &[1.0, 2.0]).expect_err("dimension mismatch");
        assert_eq!(err.code, "KNOWLEDGE_RETRIEVAL_UNAVAILABLE");
        assert_eq!(normalize_knowledge_query("  灵犀   检索 "), "灵犀 检索");
    }
}
