//! 助手配置的唯一 YAML 读取、类型验证与保留未知字段更新入口。
use super::MemoryError;
use granit_parser::{Event, Parser, ScalarStyle};
use serde_yaml_ng::{Mapping, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct AgentConfigScan {
    pub locale: Option<String>,
    pub agent_name: Option<String>,
    pub yuan: Option<String>,
    pub memory_enabled: Option<bool>,
}

pub struct AgentConfigDocument {
    value: ConfigNode,
}

fn invalid(detail: impl std::fmt::Display) -> MemoryError {
    MemoryError::InvalidRequest(format!("invalid agent config.yaml: {detail}"))
}

// 保留成熟解析器提供的每一层标签与定义位置；Value 的自动解标签不能用于配置。
#[derive(Debug, Clone, PartialEq)]
struct ConfigNode {
    value: NodeValue,
    tag: Option<String>,
}
#[derive(Debug, Clone, PartialEq)]
enum NodeValue {
    Scalar(Value),
    Sequence(Vec<ConfigNode>),
    Mapping(Vec<(ConfigNode, ConfigNode)>),
}
impl serde::Serialize for ConfigNode {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        serde::Serialize::serialize(&serde_saphyr::Tagged(&self.value, self.tag.clone()), s)
    }
}
impl serde::Serialize for NodeValue {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Scalar(value) => serde::Serialize::serialize(value, s),
            Self::Sequence(values) => serde::Serialize::serialize(values, s),
            Self::Mapping(values) => {
                use serde::ser::SerializeMap;
                let mut map = s.serialize_map(Some(values.len()))?;
                for (key, value) in values {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}
impl ConfigNode {
    fn from_value(value: Value) -> Self {
        if let Value::Tagged(tagged) = value {
            let tag = tagged.tag.to_string().trim_start_matches('!').to_string();
            let mut node = Self::from_value(tagged.value);
            node.tag = Some(tag);
            return node;
        }
        let value = match value {
            Value::Mapping(map) => NodeValue::Mapping(
                map.into_iter()
                    .map(|(k, v)| (Self::from_value(k), Self::from_value(v)))
                    .collect(),
            ),
            Value::Sequence(seq) => {
                NodeValue::Sequence(seq.into_iter().map(Self::from_value).collect())
            }
            value => NodeValue::Scalar(value),
        };
        Self { value, tag: None }
    }
    fn core_type(&self) -> Option<&str> {
        self.tag
            .as_deref()
            .and_then(|tag| tag.strip_prefix("tag:yaml.org,2002:"))
    }
    fn string(&self) -> Option<&str> {
        if self.core_type().is_some_and(|tag| tag != "str") {
            return None;
        }
        match &self.value {
            NodeValue::Scalar(Value::String(v)) => Some(v),
            _ => None,
        }
    }
    fn boolean(&self) -> Option<bool> {
        if self.core_type().is_some_and(|tag| tag != "bool") {
            return None;
        }
        match &self.value {
            NodeValue::Scalar(Value::Bool(v)) => Some(*v),
            _ => None,
        }
    }
    fn get(&self, key: &str) -> Option<&Self> {
        match &self.value {
            NodeValue::Mapping(entries) => entries
                .iter()
                .find(|(k, _)| k.string() == Some(key))
                .map(|(_, v)| v),
            _ => None,
        }
    }
    fn validate_types(&mut self) -> Result<(), MemoryError> {
        match &mut self.value {
            NodeValue::Sequence(values) => {
                for value in values.iter_mut() {
                    value.validate_types()?;
                }
            }
            NodeValue::Mapping(values) => {
                for (key, value) in values.iter_mut() {
                    key.validate_types()?;
                    value.validate_types()?;
                }
            }
            _ => {}
        }
        match self.core_type() {
            Some("binary") => {
                let NodeValue::Scalar(Value::String(raw)) = &self.value else {
                    return Err(invalid("!!binary must be a scalar"));
                };
                // 现役 binary resolver 只忽略 CR/LF，允许非规范末尾位；保存标签和原标量
                // 无需另造解码器，原读取器仍得到相同 Uint8Array，不能用更严的 codec 误拒。
                let compact: String = raw.chars().filter(|c| !matches!(c, '\r' | '\n')).collect();
                if !compact.len().is_multiple_of(4)
                    || !compact
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'/' | b'='))
                {
                    return Err(invalid("invalid !!binary under incumbent schema"));
                }
            }
            Some("timestamp") => {
                let NodeValue::Scalar(Value::String(raw)) = &self.value else {
                    return Err(invalid("!!timestamp must be a scalar"));
                };
                if !is_timestamp(raw) {
                    return Err(invalid("invalid !!timestamp value"));
                }
            }
            Some("set") => {
                let NodeValue::Mapping(entries) = &self.value else {
                    return Err(invalid("!!set must be a mapping"));
                };
                if entries
                    .iter()
                    .any(|(_, v)| !matches!(v.value, NodeValue::Scalar(Value::Null)))
                {
                    return Err(invalid("!!set values must be null"));
                }
            }
            Some("omap" | "pairs") => {
                let NodeValue::Sequence(entries) = &self.value else {
                    return Err(invalid("!!omap/!!pairs must be a sequence"));
                };
                let mut keys = Vec::new();
                for entry in entries {
                    let NodeValue::Mapping(pair) = &entry.value else {
                        return Err(invalid("!!omap/!!pairs entries must be mappings"));
                    };
                    if pair.len() != 1 {
                        return Err(invalid("!!omap/!!pairs entries must have one key"));
                    }
                    if self.core_type() == Some("omap") && keys.contains(&pair[0].0) {
                        return Err(invalid("duplicate !!omap key"));
                    }
                    keys.push(pair[0].0.clone());
                }
            }
            _ => {}
        }
        Ok(())
    }
}
// 与现役 js-yaml DEFAULT_SCHEMA 的 timestamp resolver 完全相同，只处理解析器输出的单个标量。
// 这不是对 YAML 源码做正则预处理；标签、引号、别名和层次仍由成熟解析器决定。
fn is_timestamp(value: &str) -> bool {
    static RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"^(?:[0-9]{4}-[0-9]{2}-[0-9]{2}|[0-9]{4}-[0-9]{1,2}-[0-9]{1,2}(?:[Tt]|[ \t]+)[0-9]{1,2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]*)?(?:[ \t]*(?:Z|[-+][0-9]{1,2}(?::[0-9]{2})?))?)$").expect("固定现役时间戳模式")
    });
    RE.is_match(value)
}

const OWNED_SECTIONS: [&str; 5] = [
    "agent",
    "memory",
    "experience",
    "skills",
    "workspace_context",
];

/// 只记录真正解析器输出的结构与键身份，保留合并前全部来源；不解析 YAML 文本语法。
#[derive(Default)]
struct SourceGraph {
    tags: BTreeMap<usize, String>,
    nodes: Vec<SourceNode>,
    anchors: BTreeMap<usize, usize>,
}

enum SourceNode {
    Scalar {
        key: Option<String>,
        merge: bool,
        kind: &'static str,
        value: Value,
    },
    Sequence(Vec<usize>),
    Mapping(Vec<(usize, usize)>),
}

impl SourceGraph {
    fn node<'a>(
        &mut self,
        events: &mut std::iter::Peekable<impl Iterator<Item = Event<'a>>>,
    ) -> Result<usize, MemoryError> {
        let event = events.next().ok_or_else(|| invalid("missing YAML node"))?;
        if let Event::Alias(anchor) = event {
            return self
                .anchors
                .get(&anchor)
                .copied()
                .ok_or_else(|| invalid("undefined YAML alias"));
        }
        let container_tag = event
            .tag()
            .and_then(|tag| tag.suffix_in_namespace("tag:yaml.org,2002:"))
            .map(|tag| format!("tag:yaml.org,2002:{tag}"));
        let (node, anchor) = match event {
            Event::Scalar(value, style, anchor, tag) => {
                let merge = value == "<<"
                    && (tag.as_ref().is_some_and(|tag| {
                        tag.suffix_in_namespace("tag:yaml.org,2002:").as_deref() == Some("merge")
                    }) || (tag.is_none() && style == ScalarStyle::Plain));
                let parsed = scalar_value(&value, style, tag.as_deref())?;
                let kind = match &parsed {
                    Value::String(_) => "string",
                    Value::Bool(_) => "boolean",
                    _ => "other",
                };
                let key = if merge
                    || tag.as_ref().is_some_and(|tag| {
                        tag.core_suffix() == Some("str")
                            || (tag.handle().is_empty() && tag.suffix() == "!")
                    }) {
                    Some(value.into_owned())
                } else if tag.is_some() {
                    None
                } else if style != ScalarStyle::Plain {
                    Some(value.into_owned())
                } else {
                    // 标量类型仍交由标准 YAML 实现判定，不能把数字或布尔键当字符串。
                    match &parsed {
                        Value::String(value) => Some(value.clone()),
                        _ => None,
                    }
                };
                (
                    SourceNode::Scalar {
                        key,
                        merge,
                        kind,
                        value: parsed,
                    },
                    anchor,
                )
            }
            Event::SequenceStart(_, anchor, _) => (SourceNode::Sequence(Vec::new()), anchor),
            Event::MappingStart(_, anchor, _) => (SourceNode::Mapping(Vec::new()), anchor),
            _ => return Err(invalid("unexpected YAML node")),
        };
        let id = self.nodes.len();
        self.nodes.push(node);
        if let Some(tag) = container_tag {
            self.tags.insert(id, tag);
        }
        if anchor != 0 {
            self.anchors.insert(anchor, id);
        }
        match &self.nodes[id] {
            SourceNode::Sequence(_) => {
                let mut children = Vec::new();
                while !matches!(events.peek(), Some(Event::SequenceEnd)) {
                    children.push(self.node(events)?);
                }
                events.next();
                self.nodes[id] = SourceNode::Sequence(children);
            }
            SourceNode::Mapping(_) => {
                let mut children = Vec::new();
                while !matches!(events.peek(), Some(Event::MappingEnd)) {
                    let key = self.node(events)?;
                    let value = self.node(events)?;
                    children.push((key, value));
                }
                events.next();
                self.nodes[id] = SourceNode::Mapping(children);
            }
            SourceNode::Scalar { .. } => {}
        }
        Ok(id)
    }

    fn materialize(
        &self,
        id: usize,
        active: &mut BTreeSet<usize>,
        expanded: &mut (usize, usize),
    ) -> Result<Value, MemoryError> {
        if active.len() >= 64 {
            return Err(invalid("YAML alias expansion depth budget exceeded"));
        }
        if !active.insert(id) {
            return Err(invalid("recursive YAML alias"));
        }
        expanded.0 += 1;
        if expanded.0 > MAX_NODES {
            return Err(invalid("YAML alias expansion node budget exceeded"));
        }
        let value = match &self.nodes[id] {
            SourceNode::Scalar { value, .. } => {
                expanded.1 += match value {
                    Value::String(s) => s.len(),
                    Value::Tagged(t) => t.value.as_str().map(str::len).unwrap_or(0),
                    _ => 0,
                };
                value.clone()
            }
            SourceNode::Sequence(children) => Value::Sequence(
                children
                    .iter()
                    .map(|id| self.materialize(*id, active, expanded))
                    .collect::<Result<_, _>>()?,
            ),
            SourceNode::Mapping(entries) => {
                let mut mapping = Mapping::new();
                for (key, value) in entries {
                    let mut key_value = self.materialize(*key, active, expanded)?;
                    if matches!(&self.nodes[*key],SourceNode::Scalar{merge:false,key:Some(key),..} if key=="<<")
                    {
                        key_value = Value::Tagged(Box::new(serde_yaml_ng::value::TaggedValue {
                            tag: serde_yaml_ng::value::Tag::new("tag:yaml.org,2002:str"),
                            value: Value::String("<<".into()),
                        }));
                    }
                    if mapping.contains_key(&key_value) {
                        return Err(invalid("duplicate YAML mapping key"));
                    }
                    mapping.insert(key_value, self.materialize(*value, active, expanded)?);
                }
                let mut value = Value::Mapping(mapping);
                // 键标签保留真实身份，成熟库只展开真正的 merge；子来源先处理，优先级仍由库决定。
                value.apply_merge().map_err(invalid)?;
                value
            }
        };
        active.remove(&id);
        if expanded.1 > MAX_SCALAR_BYTES {
            return Err(invalid("YAML alias expansion scalar budget exceeded"));
        }
        if let Some(tag) = self.tags.get(&id) {
            if matches!(
                tag.as_str(),
                "tag:yaml.org,2002:set" | "tag:yaml.org,2002:omap" | "tag:yaml.org,2002:pairs"
            ) {
                return Ok(Value::Tagged(Box::new(serde_yaml_ng::value::TaggedValue {
                    tag: serde_yaml_ng::value::Tag::new(tag),
                    value,
                })));
            }
        }
        Ok(value)
    }

    fn require_kind(&self, id: usize, expected: &str, location: &str) -> Result<(), MemoryError> {
        let valid = match (&self.nodes[id], expected) {
            (SourceNode::Scalar { kind, .. }, "string" | "boolean") => *kind == expected,
            (SourceNode::Sequence(values), "list")
                if self
                    .tags
                    .get(&id)
                    .is_none_or(|tag| tag == "tag:yaml.org,2002:seq") =>
            {
                values
                    .iter()
                    .all(|id| matches!(&self.nodes[*id], SourceNode::Scalar { kind: "string", .. }))
            }
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(invalid(format!("{location} must be a {expected}")))
        }
    }

    fn validate(
        &self,
        id: usize,
        section: Option<usize>,
        visited: &mut BTreeSet<(usize, Option<usize>)>,
    ) -> Result<(), MemoryError> {
        if !visited.insert((id, section)) {
            return Ok(());
        }
        if self
            .tags
            .get(&id)
            .is_some_and(|tag| tag != "tag:yaml.org,2002:map")
        {
            return Err(invalid("root/owned section must have mapping type"));
        }
        let location = section.map(|i| OWNED_SECTIONS[i]).unwrap_or("root");
        let SourceNode::Mapping(entries) = &self.nodes[id] else {
            return Err(invalid(format!("{location} must be a mapping")));
        };
        let mut seen = BTreeSet::new();
        for &(key, value) in entries {
            let SourceNode::Scalar {
                key: Some(key),
                merge,
                ..
            } = &self.nodes[key]
            else {
                return Err(invalid(format!(
                    "{location}: unsupported YAML key type; keys must be strings"
                )));
            };
            if !seen.insert((key.clone(), *merge)) {
                return Err(invalid(format!("duplicate {location} key")));
            }
            if *merge {
                // 包括被显式覆盖的来源；别名共享节点而不复制，递归由统一解析器拒绝。
                if let SourceNode::Sequence(sources) = &self.nodes[value] {
                    for &source in sources {
                        self.validate(source, section, visited)?;
                    }
                } else {
                    self.validate(value, section, visited)?;
                }
            } else if section.is_none() {
                if key == "locale" {
                    self.require_kind(value, "string", "locale")?;
                }
                if let Some(i) = OWNED_SECTIONS.iter().position(|name| name == key) {
                    self.validate(value, Some(i), visited)?;
                }
            } else {
                let expected = match (location, key.as_str()) {
                    ("agent", "name" | "yuan") => Some("string"),
                    ("memory" | "experience", "enabled") => Some("boolean"),
                    (
                        "workspace_context",
                        "discover_project_skills" | "discover_compatible_project_skills",
                    ) => Some("boolean"),
                    ("skills", "enabled" | "workspace_disabled") => Some("list"),
                    _ => None,
                };
                if let Some(expected) = expected {
                    self.require_kind(value, expected, &format!("{location}.{key}"))?;
                }
            }
        }
        Ok(())
    }
}

const MAX_CONFIG_BYTES: usize = 256 * 1024 * 1024;
const MAX_EVENTS: usize = 1_000_000;
const MAX_NODES: usize = 250_000;
const MAX_SCALAR_BYTES: usize = 64 * 1024 * 1024;
const MAX_ALIASES: usize = 50_000;
fn parser(text: &str) -> Parser<'_, granit_parser::StrInput<'_>> {
    Parser::new_from_str_with_options(
        text,
        granit_parser::options! {emit_comments:false,flow_nesting_limit:64,block_nesting_limit:64,},
    )
}
fn nonfinite_literal(value: &str) -> bool {
    matches!(
        value,
        ".inf"
            | ".Inf"
            | ".INF"
            | "+.inf"
            | "+.Inf"
            | "+.INF"
            | "-.inf"
            | "-.Inf"
            | "-.INF"
            | ".nan"
            | ".NaN"
            | ".NAN"
    )
}
fn scalar_value(
    value: &str,
    style: ScalarStyle,
    tag: Option<&granit_parser::Tag>,
) -> Result<Value, MemoryError> {
    let tag_name = tag.and_then(|tag| tag.suffix_in_namespace("tag:yaml.org,2002:"));
    match tag_name.as_deref() {
        Some("binary" | "timestamp") => {
            Ok(Value::Tagged(Box::new(serde_yaml_ng::value::TaggedValue {
                tag: serde_yaml_ng::value::Tag::new(format!(
                    "tag:yaml.org,2002:{}",
                    tag_name.unwrap()
                )),
                value: Value::String(value.to_string()),
            })))
        }
        Some("str") => Ok(Value::String(value.to_string())),
        Some("merge") if value == "<<" => Ok(Value::String(value.to_string())),
        Some("merge") => Err(invalid("invalid !!merge scalar")),
        Some("float") => {
            let yaml = serde_saphyr::to_string(&serde_saphyr::Tagged(
                value,
                Some("tag:yaml.org,2002:float".to_string()),
            ))
            .map_err(invalid)?;
            let number = serde_saphyr::from_str::<f64>(&yaml).map_err(invalid)?;
            if value.contains('_') || (!number.is_finite() && !nonfinite_literal(value)) {
                return Err(invalid("invalid !!float under incumbent schema"));
            }
            Ok(Value::Number(number.into()))
        }
        Some(core) => {
            if core == "int" && value.contains('_') {
                return Err(invalid("invalid !!int under incumbent schema"));
            }
            let yaml = serde_saphyr::to_string(&serde_saphyr::Tagged(
                value,
                Some(format!("tag:yaml.org,2002:{core}")),
            ))
            .map_err(invalid)?;
            serde_saphyr::from_str_with_options::<Value>(&yaml,serde_saphyr::options!{strict_booleans:true,reject_non_finite_typeless_float:false,}).map_err(invalid)
        }
        None if tag.is_some() || style != ScalarStyle::Plain => {
            Ok(Value::String(value.to_string()))
        }
        None if is_timestamp(value) => {
            Ok(Value::Tagged(Box::new(serde_yaml_ng::value::TaggedValue {
                tag: serde_yaml_ng::value::Tag::new("tag:yaml.org,2002:timestamp"),
                value: Value::String(value.to_string()),
            })))
        }
        None if nonfinite_literal(value) => {
            serde_yaml_ng::from_str::<Value>(value).map_err(invalid)
        }
        // 现役 js-yaml 4.3.1 数字 schema 不接收下划线或带符号的无整数部分小数。
        // 这里只对成熟解析器已分出的 plain 标量做兼容约束，不扫描或改写 YAML 源码。
        None if value.contains('_') || value.starts_with("-.") || value.starts_with("+.") => {
            Ok(Value::String(value.to_string()))
        }
        None => {
            let parsed=serde_saphyr::from_str_with_options::<Value>(value,serde_saphyr::options!{strict_booleans:true,reject_non_finite_typeless_float:false,}).map_err(invalid)?;
            // 库可能把数值溢出的 plain 标量规范成 "inf" 字符串；现役解析为原文字串。
            if matches!(parsed, Value::String(_)) {
                return Ok(Value::String(value.to_string()));
            }
            Ok(parsed)
        }
    }
}

fn validate_tag(event: &Event<'_>) -> Result<(), MemoryError> {
    let Some(tag) = event.tag() else {
        return Ok(());
    };
    if tag.handle().is_empty() && tag.suffix() == "!" {
        return Ok(());
    }
    let resolved = tag.suffix_in_namespace("tag:yaml.org,2002:");
    let valid = match event {
        Event::Scalar(..) => matches!(
            resolved.as_deref(),
            Some("str" | "bool" | "int" | "float" | "null" | "binary" | "timestamp" | "merge")
        ),
        Event::MappingStart(..) => matches!(resolved.as_deref(), Some("map" | "set")),
        Event::SequenceStart(..) => matches!(resolved.as_deref(), Some("seq" | "omap" | "pairs")),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(invalid("unsupported YAML tag or tagged node type"))
    }
}
fn parse_source(text: &str) -> Result<ConfigNode, MemoryError> {
    let mut events = Vec::new();
    let (mut count, mut nodes, mut scalar_bytes, mut aliases, mut anchors) = (0, 0, 0, 0, 0);
    for event in parser(text) {
        let (event, _) = event.map_err(invalid)?;
        count += 1;
        if count > MAX_EVENTS {
            return Err(invalid("YAML event budget exceeded"));
        }
        validate_tag(&event)?;
        match &event {
            Event::Scalar(value, _, anchor, _) => {
                nodes += 1;
                scalar_bytes += value.len();
                anchors += usize::from(*anchor != 0);
            }
            Event::MappingStart(_, anchor, _) | Event::SequenceStart(_, anchor, _) => {
                nodes += 1;
                anchors += usize::from(*anchor != 0);
            }
            Event::Alias(..) => aliases += 1,
            _ => {}
        }
        if nodes > MAX_NODES
            || scalar_bytes > MAX_SCALAR_BYTES
            || aliases > MAX_ALIASES
            || anchors > MAX_ALIASES
        {
            return Err(invalid("YAML node/scalar/alias budget exceeded"));
        }
        match event {
            Event::StreamStart
            | Event::StreamEnd
            | Event::DocumentStart(..)
            | Event::DocumentEnd
            | Event::Comment(..) => {}
            event => events.push(event),
        }
    }
    let mut graph = SourceGraph::default();
    let mut events = events.into_iter().peekable();
    let root = graph.node(&mut events)?;
    if events.next().is_some() {
        return Err(invalid("multiple YAML documents are not supported"));
    }
    graph.validate(root, None, &mut BTreeSet::new())?;
    let value = graph.materialize(root, &mut BTreeSet::new(), &mut (0, 0))?;
    let mut value = ConfigNode::from_value(value);
    value.validate_types()?;
    Ok(value)
}

impl AgentConfigDocument {
    pub fn parse(text: &str) -> Result<Self, MemoryError> {
        if text.len() > MAX_CONFIG_BYTES {
            return Err(invalid("YAML input byte budget exceeded"));
        }
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let empty = text
            .lines()
            .all(|line| line.trim().is_empty() || line.trim_start().starts_with('#'));
        let value = if empty {
            ConfigNode::from_value(Value::Mapping(Mapping::new()))
        } else {
            parse_source(text)?
        };
        let doc = Self { value };
        // 所有消费者都验证相同的敏感字段，不能按使用位置跳过错误。
        doc.scan()?;
        doc.flags()?;
        doc.skill_lists()?;
        Ok(doc)
    }

    fn field(&self, section: &str, key: &str) -> Option<&ConfigNode> {
        if section.is_empty() {
            self.value.get(key)
        } else {
            self.value.get(section)?.get(key)
        }
    }

    fn string(&self, section: &str, key: &str) -> Result<Option<String>, MemoryError> {
        self.field(section, key)
            .map(|value| {
                value
                    .string()
                    .map(str::to_string)
                    .ok_or_else(|| invalid(format!("{section}.{key} must be a string")))
            })
            .transpose()
            .map(|value| value.filter(|s| !s.trim().is_empty()))
    }

    fn boolean(&self, section: &str, key: &str) -> Result<Option<bool>, MemoryError> {
        self.field(section, key)
            .map(|value| {
                value
                    .boolean()
                    .ok_or_else(|| invalid(format!("{section}.{key} must be a boolean")))
            })
            .transpose()
    }

    fn list(&self, key: &str) -> Result<Vec<String>, MemoryError> {
        let Some(value) = self.field("skills", key) else {
            return Ok(Vec::new());
        };
        if value.core_type().is_some_and(|tag| tag != "seq") {
            return Err(invalid(format!("skills.{key} must be a string list")));
        }
        let NodeValue::Sequence(values) = &value.value else {
            return Err(invalid(format!("skills.{key} must be a string list")));
        };
        values
            .iter()
            .map(|value| {
                value
                    .string()
                    .map(str::to_string)
                    .ok_or_else(|| invalid(format!("skills.{key} must contain only strings")))
            })
            .collect()
    }

    pub fn scan(&self) -> Result<AgentConfigScan, MemoryError> {
        Ok(AgentConfigScan {
            locale: self.string("", "locale")?,
            agent_name: self.string("agent", "name")?,
            yuan: self.string("agent", "yuan")?,
            memory_enabled: self.boolean("memory", "enabled")?,
        })
    }

    pub fn flags(&self) -> Result<super::AgentConfigFlags, MemoryError> {
        Ok(super::AgentConfigFlags {
            memory_enabled: self.boolean("memory", "enabled")?,
            experience_enabled: self.boolean("experience", "enabled")?,
            discover_project_skills: self
                .boolean("workspace_context", "discover_project_skills")?,
            discover_compatible_project_skills: self
                .boolean("workspace_context", "discover_compatible_project_skills")?,
        })
    }

    pub fn skill_lists(&self) -> Result<(Vec<String>, Vec<String>), MemoryError> {
        Ok((self.list("enabled")?, self.list("workspace_disabled")?))
    }

    pub fn set(&mut self, section: &str, key: &str, value: Value) {
        let NodeValue::Mapping(root) = &mut self.value.value else {
            unreachable!("已验证根结构")
        };
        let pos = root.iter().position(|(k, _)| k.string() == Some(section));
        let pos = pos.unwrap_or_else(|| {
            root.push((
                ConfigNode::from_value(Value::String(section.into())),
                ConfigNode::from_value(Value::Mapping(Mapping::new())),
            ));
            root.len() - 1
        });
        let NodeValue::Mapping(entries) = &mut root[pos].1.value else {
            unreachable!("已验证配置分区")
        };
        if let Some((_, existing)) = entries.iter_mut().find(|(k, _)| k.string() == Some(key)) {
            *existing = ConfigNode::from_value(value);
        } else {
            entries.push((
                ConfigNode::from_value(Value::String(key.into())),
                ConfigNode::from_value(value),
            ));
        }
    }

    pub fn render(&self) -> Result<String, MemoryError> {
        let text = serde_saphyr::to_string(&self.value).map_err(invalid)?;
        Self::parse(&text)?;
        Ok(text)
    }
}

/// 文件缺失采用原默认，读取错误和损坏配置必须传给调用者。
pub fn read_agent_config(path: &std::path::Path) -> Result<AgentConfigDocument, MemoryError> {
    let text = match read_config_text(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(MemoryError::Io(format!("read agent config: {err}"))),
    };
    AgentConfigDocument::parse(&text)
}

/// 在读取阶段限制字节数，避免先无界分配再进入 YAML 预算；增长中的文件也受 take 限制。
pub fn read_config_text(path: &std::path::Path) -> std::io::Result<String> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    if file.metadata()?.len() > MAX_CONFIG_BYTES as u64 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid agent config.yaml: YAML input byte budget exceeded",
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid agent config.yaml: YAML input byte budget exceeded",
        ));
    }
    String::from_utf8(bytes).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid agent config.yaml: invalid UTF-8",
        )
    })
}
