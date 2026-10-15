//! Office 的有限只读转换：只认归档内正文，不执行宏、不加载外链或展开外部实体。
use quick_xml::{events::Event, Reader, XmlVersion};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
};

const MAX_XML_BYTES: usize = 20 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 80 * 1024 * 1024;
const MAX_NODES: usize = 200_000;
const MAX_CELLS: usize = 200_000;

#[derive(Default, Debug)]
struct Node {
    name: String,
    attrs: BTreeMap<String, String>,
    text: String,
    children: Vec<Node>,
}
impl Node {
    fn child(&self, name: &str) -> Option<&Self> {
        self.children.iter().find(|n| n.name == name)
    }
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.get(name).map(String::as_str)
    }
    fn all_text(&self) -> String {
        let mut text = self.text.clone();
        for child in &self.children {
            text.push_str(&child.all_text());
        }
        text
    }
}
fn local(raw: &str) -> String {
    raw.rsplit(':').next().unwrap_or_default().into()
}

fn xml(bytes: &[u8]) -> Result<Node, String> {
    let source = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    let mut reader = Reader::from_str(source);
    reader.config_mut().expand_empty_elements = true;
    let mut stack = vec![Node::default()];
    let mut count = 0;
    loop {
        match reader
            .read_event()
            .map_err(|e| format!("invalid Office XML: {e}"))?
        {
            Event::Start(e) => {
                count += 1;
                if count > MAX_NODES || stack.len() > 256 {
                    return Err("Office XML structure exceeds limit".into());
                }
                let mut n = Node {
                    name: local(e.name().as_ref()),
                    ..Node::default()
                };
                for attr in e.attributes() {
                    let attr = attr.map_err(|e| e.to_string())?;
                    let key = local(attr.key.as_ref());
                    let value = attr
                        .normalized_value(XmlVersion::Implicit1_0)
                        .map_err(|e| e.to_string())?
                        .into_owned();
                    if n.attrs.insert(key, value).is_some() {
                        return Err("ambiguous Office XML attribute".into());
                    }
                }
                stack.push(n);
            }
            Event::End(_) => {
                if stack.len() < 2 {
                    return Err("Office XML has unmatched end".into());
                }
                let n = stack.pop().unwrap();
                stack.last_mut().unwrap().children.push(n);
            }
            Event::Text(e) => stack
                .last_mut()
                .unwrap()
                .text
                .push_str(&e.xml_content(XmlVersion::Implicit1_0)),
            Event::CData(e) => stack
                .last_mut()
                .unwrap()
                .text
                .push_str(&e.xml_content(XmlVersion::Implicit1_0)),
            Event::GeneralRef(e) => {
                let value = if let Some(c) = e.resolve_char_ref().map_err(|e| e.to_string())? {
                    c.to_string()
                } else {
                    quick_xml::escape::resolve_predefined_entity(&e)
                        .ok_or("external or unknown XML entity refused")?
                        .into()
                };
                stack.last_mut().unwrap().text.push_str(&value);
            }
            Event::DocType(_) => {
                return Err("Office XML document types and external entities are refused".into())
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if stack.len() != 1 {
        return Err("Office XML is incomplete".into());
    }
    let mut root = stack.pop().unwrap();
    if root.children.len() != 1 || !root.text.trim().is_empty() {
        return Err("Office XML requires one document root".into());
    }
    Ok(root.children.remove(0))
}
struct Package<'a> {
    archive: zip::ZipArchive<Cursor<&'a [u8]>>,
    total: usize,
}
impl<'a> Package<'a> {
    fn new(bytes: &'a [u8]) -> Result<Self, String> {
        let archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
        if archive.len() > 4096 {
            return Err("Office archive has too many members".into());
        }
        Ok(Self { archive, total: 0 })
    }
    fn read(&mut self, path: &str) -> Result<Option<Vec<u8>>, String> {
        let mut entry = match self.archive.by_name(path) {
            Ok(e) => e,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(e) => return Err(e.to_string()),
        };
        if entry.is_dir() || entry.size() > MAX_XML_BYTES as u64 {
            return Err("Office member exceeds limit".into());
        }
        let mut bytes = Vec::new();
        (&mut entry)
            .take(MAX_XML_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        self.total = self
            .total
            .checked_add(bytes.len())
            .ok_or("Office archive size overflow")?;
        if bytes.len() > MAX_XML_BYTES || self.total > MAX_TOTAL_BYTES {
            return Err("Office expanded content exceeds limit".into());
        }
        Ok(Some(bytes))
    }
    fn required(&mut self, path: &str) -> Result<Node, String> {
        xml(&self
            .read(path)?
            .ok_or_else(|| format!("Office member missing: {path}"))?)
    }
}
pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn word_text(node: &Node, out: &mut String) {
    match node.name.as_str() {
        "t" => out.push_str(&node.text),
        "tab" => out.push('\t'),
        "br" | "cr" => out.push('\n'),
        _ => {
            for n in &node.children {
                word_text(n, out)
            }
        }
    }
}
fn word_blocks(nodes: &[Node], html: &mut String, text: &mut String) {
    for n in nodes {
        match n.name.as_str() {
            "p" => {
                let mut paragraph = String::new();
                word_text(n, &mut paragraph);
                let style = n
                    .child("pPr")
                    .and_then(|n| n.child("pStyle"))
                    .and_then(|n| n.attr("val"))
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let tag = match style.as_str() {
                    "heading1" => "h1",
                    "heading2" => "h2",
                    "heading3" => "h3",
                    "heading4" => "h4",
                    "heading5" => "h5",
                    "heading6" => "h6",
                    _ => "p",
                };
                html.push_str(&format!(
                    "<{tag}>{}</{tag}>",
                    escape(&paragraph).replace('\n', "<br>")
                ));
                text.push_str(&paragraph);
                text.push('\n');
            }
            "tbl" => {
                html.push_str("<table>");
                for row in n.children.iter().filter(|n| n.name == "tr") {
                    html.push_str("<tr>");
                    for cell in row.children.iter().filter(|n| n.name == "tc") {
                        html.push_str("<td>");
                        word_blocks(&cell.children, html, text);
                        html.push_str("</td>");
                        text.push('\t');
                    }
                    html.push_str("</tr>");
                    text.push('\n');
                }
                html.push_str("</table>");
            }
            // 内容控件只展开文档内嵌正文；不访问关系文件里的链接。
            "sdt" | "sdtContent" => word_blocks(&n.children, html, text),
            _ => {}
        }
    }
}
pub struct ConvertedDocument {
    pub html: String,
    pub text: String,
}
pub fn docx(bytes: &[u8]) -> Result<ConvertedDocument, String> {
    let mut package = Package::new(bytes)?;
    let document = package.required("word/document.xml")?;
    if document.name != "document" {
        return Err("docx document root missing".into());
    }
    let body = document.child("body").ok_or("docx body missing")?;
    let (mut html, mut text) = (String::new(), String::new());
    word_blocks(&body.children, &mut html, &mut text);
    Ok(ConvertedDocument { html, text })
}
fn part_path(target: &str) -> Result<String, String> {
    if target.contains('\\') || target.contains(':') || target.contains(['?', '#']) {
        return Err("invalid worksheet relationship".into());
    }
    let base = if target.starts_with('/') {
        target.trim_start_matches('/').to_string()
    } else {
        format!("xl/{target}")
    };
    let mut parts = Vec::new();
    for part in base.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err("worksheet escapes package".into());
                }
            }
            _ => parts.push(part),
        }
    }
    if parts.first() != Some(&"xl") {
        return Err("worksheet must remain in workbook package".into());
    }
    Ok(parts.join("/"))
}
fn column(reference: &str) -> Result<usize, String> {
    let mut result = 0usize;
    for b in reference.bytes().take_while(u8::is_ascii_alphabetic) {
        result = result
            .checked_mul(26)
            .and_then(|n| n.checked_add((b.to_ascii_uppercase() - b'A' + 1) as usize))
            .ok_or("worksheet column overflow")?;
    }
    if result == 0 || result > 16_384 {
        return Err("invalid worksheet column".into());
    }
    Ok(result)
}

fn date_format(format: &str) -> bool {
    // 与现役 ExcelJS isDateFmt 一致，引号文字与方括号修饰不当作日期字符。
    let mut quoted = false;
    let mut bracket = false;
    for c in format.chars() {
        if c == '"' && !bracket {
            quoted = !quoted;
            continue;
        }
        if c == '[' && !quoted {
            bracket = true;
            continue;
        }
        if c == ']' && !quoted {
            bracket = false;
            continue;
        }
        if !quoted && !bracket && matches!(c, 'y' | 'm' | 'd' | 'h' | 'M' | 's' | 'b') {
            return true;
        }
    }
    false
}
fn date_styles(package: &mut Package<'_>) -> Result<Vec<bool>, String> {
    let Some(bytes) = package.read("xl/styles.xml")? else {
        return Ok(vec![false]);
    };
    let styles = xml(&bytes)?;
    let mut custom = BTreeMap::new();
    if let Some(formats) = styles.child("numFmts") {
        for format in formats.children.iter().filter(|n| n.name == "numFmt") {
            let id = format
                .attr("numFmtId")
                .ok_or("number format id missing")?
                .parse::<u32>()
                .map_err(|_| "invalid number format id")?;
            if custom
                .insert(
                    id,
                    date_format(
                        format
                            .attr("formatCode")
                            .ok_or("number format code missing")?,
                    ),
                )
                .is_some()
            {
                return Err("duplicate number format id".into());
            }
        }
    }
    let Some(formats) = styles.child("cellXfs") else {
        return Ok(vec![false]);
    };
    formats
        .children
        .iter()
        .filter(|n| n.name == "xf")
        .map(|format| {
            let id = format
                .attr("numFmtId")
                .unwrap_or("0")
                .parse::<u32>()
                .map_err(|_| "invalid cell number format")?;
            Ok(custom
                .get(&id)
                .copied()
                .unwrap_or(matches!(id,14..=22|27..=36|45..=47|50..=58)))
        })
        .collect()
}
fn excel_date(value: &str, date1904: bool) -> Result<String, String> {
    let serial = value
        .parse::<f64>()
        .map_err(|_| "invalid Excel date number")?;
    // 保持现役 ExcelJS excelToDate 的1900闰日兼容和1904偏移，再明确呈现UTC。
    let raw_millis = (serial - 25_569.0 + if date1904 { 1_462.0 } else { 0.0 }) * 86_400_000.0;
    // JavaScript Math.round 在负半毫秒处朝正方向取整，保持现役日期转换。
    let lower = raw_millis.floor();
    let millis = if raw_millis - lower < 0.5 {
        lower
    } else {
        lower + 1.0
    };
    if !millis.is_finite() || millis.abs() > 8_640_000_000_000_000.0 {
        return Err("Excel date exceeds supported range".into());
    }
    let date = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(millis as i64)
        .ok_or("Excel date exceeds supported range")?;
    Ok(date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}
pub fn xlsx(bytes: &[u8]) -> Result<ConvertedDocument, String> {
    xlsx_sheet(&mut Package::new(bytes)?, 0)
}

/// 工具读取按现役上限提取前八张表；所有解压共用同一预算，超出表数明确返回。
pub fn xlsx_for_read(bytes: &[u8]) -> Result<(String, usize), String> {
    let mut package = Package::new(bytes)?;
    let workbook = package.required("xl/workbook.xml")?;
    let sheets: Vec<_> = workbook
        .child("sheets")
        .ok_or("xlsx worksheets missing")?
        .children
        .iter()
        .filter(|n| n.name == "sheet")
        .collect();
    if sheets.is_empty() {
        return Err("xlsx worksheets missing".into());
    }
    let mut text = String::new();
    for (index, sheet) in sheets.iter().take(8).enumerate() {
        match xlsx_sheet(&mut package, index) {
            Ok(document) => {
                text.push_str(&format!(
                    "[Sheet: {}]\n{}\n",
                    sheet.attr("name").unwrap_or("unnamed"),
                    document.text
                ));
            }
            Err(error) if error == "xlsx has no rows" => {}
            Err(error) => return Err(error),
        }
    }
    let omitted = sheets.len().saturating_sub(8);
    if omitted > 0 {
        text.push_str(&format!(
            "[Workbook truncated: showing 8/{} sheets]",
            sheets.len()
        ));
    }
    Ok((text, omitted))
}

fn xlsx_sheet(package: &mut Package<'_>, index: usize) -> Result<ConvertedDocument, String> {
    let workbook = package.required("xl/workbook.xml")?;
    let date1904 = match workbook
        .child("workbookPr")
        .and_then(|n| n.attr("date1904"))
    {
        None | Some("0" | "false") => false,
        Some("1" | "true") => true,
        _ => return Err("invalid workbook calendar".into()),
    };
    let date_styles = date_styles(package)?;
    let first = workbook
        .child("sheets")
        .and_then(|n| n.children.iter().filter(|n| n.name == "sheet").nth(index))
        .ok_or("xlsx has no rows")?;
    let id = first.attr("id").ok_or("xlsx worksheet identity missing")?;
    let rels = package.required("xl/_rels/workbook.xml.rels")?;
    let matching: Vec<_> = rels
        .children
        .iter()
        .filter(|r| r.name == "Relationship" && r.attr("Id") == Some(id))
        .collect();
    if matching.len() != 1 {
        return Err("xlsx worksheet relationship missing or duplicated".into());
    }
    let relationship = matching[0];
    if relationship
        .attr("TargetMode")
        .is_some_and(|mode| mode != "Internal")
        || !relationship
            .attr("Type")
            .is_some_and(|t| t.ends_with("/worksheet"))
    {
        return Err("external or invalid worksheet refused".into());
    }
    let path = part_path(
        relationship
            .attr("Target")
            .ok_or("worksheet path missing")?,
    )?;
    let shared = match package.read("xl/sharedStrings.xml")? {
        Some(bytes) => xml(&bytes)?
            .children
            .iter()
            .filter(|n| n.name == "si")
            .map(Node::all_text)
            .collect::<Vec<_>>(),
        None => Vec::new(),
    };
    let sheet = package.required(&path)?;
    let rows = sheet.child("sheetData").ok_or("xlsx has no rows")?;
    let mut rendered = Vec::new();
    let mut width = 0;
    let mut cells = 0;
    for row in rows.children.iter().filter(|n| n.name == "row") {
        let mut values = BTreeMap::new();
        let mut inferred = 0;
        for cell in row.children.iter().filter(|n| n.name == "c") {
            cells += 1;
            if cells > MAX_CELLS {
                return Err("worksheet exceeds cell limit".into());
            }
            let col = if let Some(reference) = cell.attr("r") {
                column(reference)?
            } else {
                inferred + 1
            };
            inferred = col;
            if col > 16_384 {
                return Err("worksheet exceeds column limit".into());
            }
            let value = cell.child("v").map(|n| n.text.as_str()).unwrap_or("");
            let value = match cell.attr("t").unwrap_or("") {
                "s" => shared
                    .get(
                        value
                            .parse::<usize>()
                            .map_err(|_| "invalid shared string index")?,
                    )
                    .ok_or("shared string missing")?
                    .clone(),
                "inlineStr" => cell.child("is").map(Node::all_text).unwrap_or_default(),
                "b" => match value {
                    "1" => "true".into(),
                    "0" => "false".into(),
                    _ => return Err("invalid boolean cell".into()),
                },
                "d" => chrono::DateTime::parse_from_rfc3339(value)
                    .map_err(|_| "invalid ISO worksheet date")?
                    .with_timezone(&chrono::Utc)
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                "" | "n" if !value.is_empty() => {
                    let style = cell
                        .attr("s")
                        .unwrap_or("0")
                        .parse::<usize>()
                        .map_err(|_| "invalid cell style")?;
                    let is_date = *date_styles
                        .get(style)
                        .ok_or("worksheet cell style missing")?;
                    if is_date {
                        excel_date(value, date1904)?
                    } else {
                        value.to_string()
                    }
                }
                _ => value.to_string(),
            };
            width = width.max(col);
            if values.insert(col, value).is_some() {
                return Err("duplicate worksheet cell".into());
            }
        }
        if !values.is_empty() {
            rendered.push(values);
        }
    }
    if rendered.is_empty() {
        return Err("xlsx has no rows".into());
    }
    if width
        .checked_mul(rendered.len())
        .is_none_or(|n| n > MAX_CELLS)
    {
        return Err("worksheet display exceeds cell limit".into());
    }
    let (mut html, mut text) = (String::from("<table>"), String::new());
    for row in rendered {
        html.push_str("<tr>");
        for col in 1..=width {
            let value = row.get(&col).map(String::as_str).unwrap_or("");
            html.push_str(&format!("<td>{}</td>", escape(value)));
            if col > 1 {
                text.push('\t');
            }
            text.push_str(value);
        }
        html.push_str("</tr>");
        text.push('\n');
    }
    html.push_str("</table>");
    Ok(ConvertedDocument { html, text })
}
