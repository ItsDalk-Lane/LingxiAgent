//! 助手角色卡真实文件读写面：标准 ZIP、持久导入计划及原有人格/记忆/技能存储。
use crate::ServiceState;
use axum::{
    body::Bytes,
    extract::{FromRequest, Multipart, Path as RoutePath, Request, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use base64::Engine as _;
use lingxi_adapters::storage::{agent_memory as am, skill_library as sl};
use lingxi_kernel::memory::{parse_skill_metadata, sanitize_skill_name};
use serde_json::{json, Map, Value};
use std::{
    collections::{BTreeMap, HashSet},
    io::{Cursor, Read, Write},
    path::{Component, Path, PathBuf},
};

const MAX_PACKAGE_BYTES: u64 = 80 * 1024 * 1024;
const MAX_ENTRIES: usize = 10_000;
pub(crate) static CARD_APPLICATION_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn fail(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({"error": message.into()}))).into_response()
}
fn error(message: impl Into<String>) -> Response {
    fail(StatusCode::INTERNAL_SERVER_ERROR, message)
}
fn bad(message: impl Into<String>) -> Response {
    fail(StatusCode::BAD_REQUEST, message)
}
fn checked_file(path: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect package file: {error}"))?;
    if !metadata.file_type().is_file() {
        return Err("package file must be a regular file".into());
    }
    Ok(())
}
// 先检查句柄对应实物，再受限读取；路径在检查后被替换也不能跟随符号链接或无限分配。
fn bounded_file(path: &Path) -> Result<Vec<u8>, String> {
    checked_file(path)?;
    if std::fs::symlink_metadata(path)
        .map_err(|error| error.to_string())?
        .len()
        > MAX_PACKAGE_BYTES
    {
        return Err("package size exceeds limit".into());
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000);
    }
    let file = options.open(path).map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_PACKAGE_BYTES {
        return Err("package size exceeds limit or is not regular".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("package links are forbidden".into());
        }
    }
    let mut bytes = Vec::new();
    file.take(MAX_PACKAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_PACKAGE_BYTES {
        return Err("package size exceeds limit".into());
    }
    Ok(bytes)
}
fn charge(total: &mut u64, size: u64) -> Result<(), String> {
    *total = total.checked_add(size).ok_or("package size overflow")?;
    if *total > MAX_PACKAGE_BYTES {
        return Err("package size exceeds limit".into());
    }
    Ok(())
}
fn inside(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if relative.is_empty()
        || relative.contains('\\')
        || relative.contains(':')
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("package path must be a safe relative path".into());
    }
    let result = root.join(path);
    let mut current = root.to_path_buf();
    for part in path.components() {
        current.push(part.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err("package links are forbidden".into())
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("cannot inspect package path: {error}")),
        }
    }
    Ok(result)
}
fn token_root(state: &ServiceState, token: &str) -> Result<PathBuf, String> {
    if token.len() != 32
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err("invalid character card token".into());
    }
    inside(
        state.memory().home(),
        &format!(".ephemeral/character-card-imports/{token}"),
    )
}
fn create_stage(state: &ServiceState) -> Result<(String, PathBuf), String> {
    let token = crate::auth::random_base64url(24)
        .map_err(|error| format!("cannot allocate character card token: {error}"))?;
    let root = token_root(state, &token)?;
    std::fs::create_dir_all(root.join("package")).map_err(|error| error.to_string())?;
    Ok((token, root))
}
fn extract_zip(bytes: &[u8], root: &Path) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| format!("invalid character card ZIP: {error}"))?;
    if zip.len() > MAX_ENTRIES {
        return Err("character card ZIP contains too many entries".into());
    }
    let mut total = 0u64;
    let mut names = HashSet::new();
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|error| error.to_string())?;
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("ZIP links are forbidden".into());
        }
        let name = entry
            .name()
            .map_err(|error| error.to_string())?
            .into_owned();
        let relative = name.trim_end_matches('/');
        let destination = inside(root, relative)?;
        if !names.insert(relative.to_string()) {
            return Err("ZIP duplicate paths are forbidden".into());
        }
        total = total
            .checked_add(entry.size())
            .ok_or("ZIP expanded size overflow")?;
        if total > MAX_PACKAGE_BYTES {
            return Err("character card ZIP expands beyond limit".into());
        }
        if entry.is_dir() {
            std::fs::create_dir_all(destination).map_err(|error| error.to_string())?;
            continue;
        }
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(|error| error.to_string())?;
        let written = std::io::copy(&mut entry.by_ref().take(MAX_PACKAGE_BYTES + 1), &mut output)
            .map_err(|error| error.to_string())?;
        if written != entry.size() {
            return Err("ZIP entry length does not match declaration".into());
        }
    }
    Ok(())
}
fn structured_file(path: &Path) -> Result<Value, String> {
    checked_file(path)?;
    if path.extension().and_then(|part| part.to_str()) == Some("json") {
        serde_json::from_slice(&std::fs::read(path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("invalid character card JSON: {error}"))
    } else {
        am::config::read_yaml_mapping_as_json(path)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "character card YAML is empty".into())
    }
}
fn descriptor(root: &Path) -> Result<Value, String> {
    for name in [
        "manifest.json",
        "manifest.yaml",
        "manifest.yml",
        "character-card.json",
        "character-card.yaml",
        "character-card.yml",
        "card.json",
        "card.yaml",
        "card.yml",
    ] {
        let path = root.join(name);
        if !path.exists() {
            continue;
        }
        let manifest = structured_file(&path)?;
        return if let Some(relative) = manifest.get("card").and_then(Value::as_str) {
            structured_file(&inside(root, relative)?)
        } else {
            Ok(manifest)
        };
    }
    Err("character card descriptor not found".into())
}
fn first_text(values: &[&Value]) -> String {
    values
        .iter()
        .find_map(|value| {
            value
                .as_str()
                .filter(|text| !text.trim().is_empty())
                .map(|text| text.trim().to_string())
        })
        .unwrap_or_default()
}
fn image_mime(path: &Path) -> Result<&'static str, String> {
    match path
        .extension()
        .and_then(|part| part.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => Ok("image/png"),
        Some("jpg" | "jpeg") => Ok("image/jpeg"),
        Some("webp") => Ok("image/webp"),
        Some("gif") => Ok("image/gif"),
        _ => Err("character card asset has unsupported image type".into()),
    }
}
fn normalize(card: &Value, root: &Path, token: &str, name: &str) -> Result<Value, String> {
    if !card.is_object() {
        return Err("character card must be an object".into());
    }
    let agent = card
        .get("agent")
        .or_else(|| card.get("character"))
        .unwrap_or(&Value::Null);
    let agent_name = first_text(&[&agent["name"], &card["name"]]);
    if agent_name.is_empty() {
        return Err("agent.name is required".into());
    }
    let id = first_text(&[&agent["id"], &card["id"]]);
    if !id.is_empty() {
        am::validate_agent_id(&id).map_err(|error| error.to_string())?;
    }
    let raw_yuan = first_text(&[&agent["yuan"], &card["yuan"]]);
    let yuan = if ["lingxi", "butter", "ming", "kong"].contains(&raw_yuan.as_str()) {
        raw_yuan.as_str()
    } else {
        "lingxi"
    };
    let prompts = &card["prompts"];
    let identity = first_text(&[
        &prompts["identity"],
        &card["identity"]["content"],
        &card["identity"]["prompt"],
        &card["identity"],
    ]);
    let agents = first_text(&[
        &prompts["agents"],
        &prompts["ishiki"],
        &prompts["yuan"],
        &card["agents"],
        &card["ishiki"],
    ]);
    let public = first_text(&[
        &prompts["publicAgents"],
        &prompts["publicIshiki"],
        &prompts["public_ishiki"],
        &card["publicAgents"],
        &card["publicIshiki"],
    ]);
    let compiled = &card["memory"]["compiled"];
    let compiled = json!({"facts": first_text(&[&compiled["facts"], &card["memory"]["compiledFacts"], &card["memory"]["factsText"]]),
        "today": first_text(&[&compiled["today"], &card["memory"]["today"], &card["todayMemory"]]),
        "week": first_text(&[&compiled["week"], &card["memory"]["week"], &card["weekMemory"]]),
        "longterm": first_text(&[&compiled["longterm"], &compiled["longTerm"], &card["memory"]["longterm"], &card["longtermMemory"]])});
    let mut facts = Vec::new();
    let raw_facts = card["memory"]
        .get("facts")
        .or_else(|| card.get("facts"))
        .or_else(|| card.get("memories"));
    if let Some(entries) = raw_facts.and_then(Value::as_array) {
        for entry in entries {
            let fact = first_text(&[&entry["fact"], &entry["content"], &entry["text"]]);
            if !fact.is_empty() {
                facts.push(json!({"fact": fact, "tags": entry["tags"].as_array().map(|tags| tags.iter().filter(|tag| tag.is_string()).cloned().collect::<Vec<_>>()).unwrap_or_default(), "time": entry.get("time").or_else(|| entry.get("date")).cloned().unwrap_or(Value::Null), "session_id": entry["session_id"].as_str().unwrap_or("character-card-import")}));
            }
        }
    }
    let mut assets = Map::new();
    for (key, aliases) in [
        ("avatar", vec!["avatar", "portrait"]),
        ("cardFront", vec!["cardFront", "card_front", "front"]),
        ("cardBack", vec!["cardBack", "card_back", "back"]),
        ("yuanIcon", vec!["yuanIcon", "yuan_icon"]),
    ] {
        if let Some(relative) = aliases
            .iter()
            .find_map(|alias| card["assets"][alias].as_str())
            .filter(|relative| !relative.is_empty())
        {
            let path = inside(root, relative)?;
            checked_file(&path)?;
            image_mime(&path)?;
            assets.insert(key.into(), json!(relative));
        }
    }
    let mut skills = Vec::new();
    let mut bundles = card["skills"]["bundles"]
        .as_array()
        .cloned()
        .or_else(|| card["skillBundles"].as_array().cloned())
        .unwrap_or_default();
    if let Some(items) = card["skills"]
        .as_array()
        .or_else(|| card["skills"]["items"].as_array())
    {
        bundles.push(json!({"name": "Imported Skills", "skills": items}));
    }
    let mut skill_sources = HashSet::new();
    let mut skill_names = HashSet::new();
    for bundle in bundles {
        for skill in bundle["skills"].as_array().cloned().unwrap_or_default() {
            let relative = skill
                .as_str()
                .or_else(|| skill["path"].as_str())
                .ok_or("skill path is required")?;
            let path = inside(root, relative)?.join("SKILL.md");
            checked_file(&path)?;
            let content = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
            let name = parse_skill_metadata(&content, "")
                .name
                .and_then(|name| sanitize_skill_name(&name))
                .ok_or("skill name is invalid")?;
            let canonical = path
                .parent()
                .ok_or("skill directory missing")?
                .canonicalize()
                .map_err(|error| error.to_string())?;
            if !skill_sources.insert(canonical) || !skill_names.insert(name.clone()) {
                return Err("duplicate skill source or install name".into());
            }
            skills.push(json!({"name": name, "path": relative, "bundle": bundle["name"].as_str().unwrap_or("Skill Bundle")}));
        }
    }
    Ok(
        json!({"token": token, "packageName": first_text(&[&card["package"]["name"], &card["name"], &json!(name)]),
        "agent": {"id": id, "name": agent_name, "yuan": yuan, "description": first_text(&[&agent["description"], &card["description"]])},
        "prompts": {"identity": identity, "agents": agents, "publicAgents": public}, "memoryFacts": facts, "memoryCompiled": compiled, "assets": assets, "skills": skills}),
    )
}
fn preview(plan: &Value, agent_id: Option<&str>) -> Value {
    let facts = plan["memoryFacts"].as_array().cloned().unwrap_or_default();
    let compiled = &plan["memoryCompiled"];
    let count = facts.len()
        + ["facts", "today", "week", "longterm"]
            .iter()
            .filter(|key| {
                compiled[*key]
                    .as_str()
                    .is_some_and(|text| !text.trim().is_empty())
            })
            .count();
    let mut bundles: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for skill in plan["skills"].as_array().cloned().unwrap_or_default() {
        bundles
            .entry(skill["bundle"].as_str().unwrap_or("Skill Bundle").into())
            .or_default()
            .push(json!({"name": skill["name"]}));
    }
    let bundle_list = bundles
        .into_iter()
        .map(|(name, skills)| json!({"name": name, "skillCount": skills.len(), "skills": skills}))
        .collect::<Vec<_>>();
    let text = first_text(&[
        &compiled["facts"],
        &facts
            .first()
            .map(|entry| entry["fact"].clone())
            .unwrap_or(Value::Null),
        &compiled["today"],
        &compiled["week"],
        &compiled["longterm"],
    ]);
    let mut result = json!({"mode": if agent_id.is_some() {"export"} else {"import"}, "packageName": plan["packageName"], "agent": plan["agent"], "prompts": plan["prompts"], "memory": {"available": count > 0, "count": count, "preview": text.chars().take(240).collect::<String>(), "compiled": compiled}, "skills": {"count": plan["skills"].as_array().map_or(0, Vec::len), "bundles": bundle_list}, "assets": plan["assets"].as_object().map(|assets| assets.keys().map(|key| (key.clone(), json!(true))).collect::<Map<_,_>>()).unwrap_or_default()});
    if let Some(id) = agent_id {
        result["agentId"] = json!(id);
    } else {
        result["token"] = plan["token"].clone();
    }
    result
}
fn load_plan(state: &ServiceState, token: &str) -> Result<(Value, PathBuf), String> {
    let root = token_root(state, token)?;
    let path = root.join("plan.json");
    checked_file(&path)?;
    let plan: Value =
        serde_json::from_slice(&std::fs::read(path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    if plan["token"] != token {
        return Err("character card plan token mismatch".into());
    }
    Ok((plan, root))
}
fn save_plan(root: &Path, plan: &Value) -> Result<(), String> {
    crate::paths::atomic_write_private(
        &root.join("plan.json"),
        serde_json::to_vec(plan)
            .map_err(|error| error.to_string())?
            .as_slice(),
    )
    .map_err(|error| error.to_string())
}

pub async fn plan_route(State(state): State<ServiceState>, request: Request) -> Response {
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|header| header.to_str().ok())
        .unwrap_or("")
        .to_string();
    let mut original_name = "Character Card".to_string();
    let bytes = if content_type.starts_with("multipart/form-data") {
        let mut multipart = match Multipart::from_request(request, &state).await {
            Ok(value) => value,
            Err(error) => return bad(error.to_string()),
        };
        let mut file = None;
        loop {
            let field = match multipart.next_field().await {
                Ok(Some(field)) => field,
                Ok(None) => break,
                Err(error) => return bad(error.to_string()),
            };
            if field.name() != Some("file") {
                return bad("unexpected character card upload field");
            }
            if file.is_some() {
                return bad("only one character card file is allowed");
            }
            original_name = field.file_name().unwrap_or("card.json").to_string();
            file = Some(match field.bytes().await {
                Ok(bytes) => bytes,
                Err(error) => return bad(error.to_string()),
            });
        }
        match file {
            Some(bytes) => bytes,
            None => return bad("file is required"),
        }
    } else {
        let bytes =
            match axum::body::to_bytes(request.into_body(), MAX_PACKAGE_BYTES as usize).await {
                Ok(value) => value,
                Err(error) => return bad(error.to_string()),
            };
        let body: Value = match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(error) => return bad(error.to_string()),
        };
        let Some(path) = body["path"].as_str() else {
            return bad("file or absolute path is required");
        };
        let path = Path::new(path);
        if !path.is_absolute() {
            return bad("character card path must be absolute");
        }
        if let Err(error) = checked_file(path) {
            return bad(error);
        }
        original_name = body["name"]
            .as_str()
            .or_else(|| path.file_name().and_then(|name| name.to_str()))
            .unwrap_or("card.json")
            .into();
        match bounded_file(path) {
            Ok(value) => Bytes::from(value),
            Err(message) => return bad(message),
        }
    };
    if bytes.len() as u64 > MAX_PACKAGE_BYTES {
        return bad("character card upload exceeds limit");
    }
    let (token, root) = match create_stage(&state) {
        Ok(value) => value,
        Err(message) => return error(message),
    };
    let package = root.join("package");
    let result = (|| {
        if bytes.starts_with(b"PK") {
            extract_zip(&bytes, &package)?;
        } else {
            let extension = Path::new(&original_name)
                .extension()
                .and_then(|part| part.to_str())
                .unwrap_or("json")
                .to_ascii_lowercase();
            if !["json", "yaml", "yml"].contains(&extension.as_str()) {
                return Err("unsupported character card package type".into());
            }
            std::fs::write(package.join(format!("card.{extension}")), &bytes)
                .map_err(|error| error.to_string())?;
        }
        let card = descriptor(&package)?;
        let plan = normalize(&card, &package, &token, &original_name)?;
        save_plan(&root, &plan)?;
        Ok::<_, String>(plan)
    })();
    match result {
        Ok(plan) => Json(json!({"plan": preview(&plan, None)})).into_response(),
        Err(message) => {
            if let Err(cleanup) = std::fs::remove_dir_all(&root) {
                return error(format!(
                    "{message}; incomplete import staging cleanup: {cleanup}"
                ));
            }
            bad(message)
        }
    }
}

async fn response_json(response: Response) -> Result<Value, String> {
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .map_err(|error| error.to_string())?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if !status.is_success() || value.get("error").is_some() {
        return Err(format!("{status}: {}", value["error"]));
    }
    Ok(value)
}
async fn asset_sources(
    state: &ServiceState,
    agent_id: &str,
    yuan: &str,
) -> Result<Map<String, Value>, String> {
    state
        .memory()
        .agent_dir_checked(agent_id)
        .map_err(|error| error.to_string())?;
    let mut assets = Map::new();
    let names = match yuan {
        "butter" => (
            "Butter.png",
            "yuan-butter-card-back.png",
            "yuan-butter-emblem.png",
        ),
        "ming" => (
            "Ming.png",
            "yuan-ming-card-back.png",
            "yuan-ming-emblem.png",
        ),
        "kong" => (
            "Kong.png",
            "yuan-kong-card-back.png",
            "yuan-kong-emblem.png",
        ),
        _ => (
            "Lingxi.png",
            "yuan-lingxi-card-back.png",
            "yuan-lingxi-emblem.png",
        ),
    };
    let product = state.skills().product_dir();
    let static_root = product.join("desktop/src/assets");
    let static_root = if static_root.is_dir() {
        static_root
    } else {
        product
            .parent()
            .unwrap_or(product)
            .join("desktop/src/assets")
    };
    // 与现役头像读面共用优先顺序，包含新裁剪头像的 avatars 目录。
    let found = response_json(
        crate::persona::avatar_path_route(
            State(state.clone()),
            RoutePath(agent_id.into()),
            axum::extract::RawQuery(None),
        )
        .await,
    )
    .await?;
    let avatar = found["path"]
        .as_str()
        .map(PathBuf::from)
        .or_else(|| Some(static_root.join(names.0)).filter(|path| path.exists()));
    for (key, path) in [
        ("avatar", avatar.clone()),
        ("cardFront", avatar),
        (
            "cardBack",
            Some(static_root.join("character-cards").join(names.1)),
        ),
        (
            "yuanIcon",
            Some(static_root.join("character-cards").join(names.2)),
        ),
    ] {
        if let Some(path) = path.filter(|path| path.exists()) {
            checked_file(&path)?;
            image_mime(&path)?;
            assets.insert(key.into(), json!(path));
        }
    }
    Ok(assets)
}
async fn export_source(state: &ServiceState, id: &str) -> Result<Value, String> {
    let config = response_json(
        crate::agents_admin::agent_config_readback::agent_config_get_route(
            State(state.clone()),
            RoutePath(id.into()),
        )
        .await,
    )
    .await?;
    let identity = response_json(
        crate::persona::identity_get_route(State(state.clone()), RoutePath(id.into())).await,
    )
    .await?;
    let agents = response_json(
        crate::persona::agents_md_get_route(State(state.clone()), RoutePath(id.into())).await,
    )
    .await?;
    let public = response_json(
        crate::persona::public_agents_md_get_route(State(state.clone()), RoutePath(id.into()))
            .await,
    )
    .await?;
    let compiled = state
        .memory()
        .compiled_store(id)
        .map_err(|error| error.to_string())?
        .read_sections()
        .map_err(|error| error.to_string())?;
    let facts = state.memory().fact_store(id).map_err(|error| error.to_string())?.export_all().map_err(|error| error.to_string())?.into_iter().map(|row| json!({"fact": row.fact, "tags": row.tags, "time": row.time, "session_id": row.session_id})).collect::<Vec<_>>();
    let mut skills = Vec::new();
    for skill in state
        .skills()
        .list_for_agent(id, false, None)
        .map_err(|error| error.to_string())?
    {
        if skill["enabled"] == true && !(skill["readonly"] == true && skill["source"] == "external")
        {
            let path = skill["baseDir"]
                .as_str()
                .ok_or("export skill has no directory")?;
            checked_file(&Path::new(path).join("SKILL.md"))?;
            skills.push(json!({"name": skill["name"], "path": path, "bundle": format!("{} Bundle", config["agent"]["name"].as_str().unwrap_or(id))}));
        }
    }
    let yuan = config["agent"]["yuan"].as_str().unwrap_or("lingxi");
    let assets = asset_sources(state, id, yuan).await?;
    Ok(
        json!({"agent": config["agent"], "packageName": format!("{}-charactercard.zip", config["agent"]["name"].as_str().unwrap_or(id)), "prompts": {"identity": identity["content"], "agents": agents["content"], "publicAgents": public["content"]}, "memoryFacts": facts, "memoryCompiled": {"facts": compiled.facts, "today": compiled.today, "week": compiled.week, "longterm": compiled.longterm}, "assets": assets, "skills": skills}),
    )
}

pub async fn export_preview_route(
    State(state): State<ServiceState>,
    Json(body): Json<Value>,
) -> Response {
    let Some(id) = body["agentId"].as_str() else {
        return bad("agentId is required");
    };
    match export_source(&state, id).await {
        Ok(plan) => Json(json!({"plan": preview(&plan, Some(id))})).into_response(),
        Err(message) => error(message),
    }
}
fn files_under(root: &Path) -> Result<Vec<PathBuf>, String> {
    let metadata = std::fs::symlink_metadata(root).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_dir() {
        return Err("package root must be a real directory".into());
    }
    let mut files = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(dir).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let metadata = entry.file_type().map_err(|error| error.to_string())?;
            if metadata.is_symlink() {
                return Err("package links are forbidden".into());
            }
            if metadata.is_dir() {
                dirs.push(entry.path());
            } else if metadata.is_file() {
                files.push(entry.path());
            } else {
                return Err("package contains special files".into());
            }
            if dirs.len() + files.len() > MAX_ENTRIES {
                return Err("package contains too many entries".into());
            }
        }
    }
    files.sort();
    Ok(files)
}
fn copy_tree(source: &Path, destination: &Path, total: &mut u64) -> Result<(), String> {
    std::fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    for path in files_under(source)? {
        let relative = path
            .strip_prefix(source)
            .map_err(|error| error.to_string())?;
        let target = inside(
            destination,
            relative.to_str().ok_or("non-UTF8 package path")?,
        )?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let bytes = bounded_file(&path)?;
        charge(total, bytes.len() as u64)?;
        std::fs::write(target, bytes).map_err(|error| error.to_string())?;
    }
    Ok(())
}
pub async fn export_route(State(state): State<ServiceState>, Json(body): Json<Value>) -> Response {
    let Some(id) = body["agentId"].as_str() else {
        return bad("agentId is required");
    };
    let plan = match export_source(&state, id).await {
        Ok(plan) => plan,
        Err(message) => return error(message),
    };
    let (_, stage) = match create_stage(&state) {
        Ok(value) => value,
        Err(message) => return error(message),
    };
    let package = stage.join("package");
    let target = state
        .skills()
        .workspace_root()
        .unwrap_or_else(|| state.memory().home().join("exports"));
    let mut published: Option<PathBuf> = None;
    let result = (|| {
        std::fs::create_dir_all(&target).map_err(|error| error.to_string())?;
        let mut card = json!({"kind": "CharacterCard", "schemaVersion": 1, "package": {"name": plan["packageName"]}, "agent": plan["agent"], "prompts": plan["prompts"], "assets": {}, "skills": {"bundles": []}});
        let mut copies: Vec<(PathBuf, PathBuf)> = Vec::new();
        let mut budget = 0;
        let mut sources = HashSet::new();
        let mut targets = HashSet::new();
        let mut add_copy = |source: PathBuf, target: PathBuf| -> Result<(), String> {
            if copies.len() + 2 > MAX_ENTRIES {
                return Err("package contains too many entries".into());
            }
            checked_file(&source)?;
            if !sources.insert(source.canonicalize().map_err(|error| error.to_string())?)
                || !targets.insert(target.clone())
            {
                return Err("duplicate export material source or destination".into());
            }
            charge(
                &mut budget,
                std::fs::metadata(&source)
                    .map_err(|error| error.to_string())?
                    .len(),
            )?;
            copies.push((source, target));
            Ok(())
        };
        let mut asset_sources: BTreeMap<PathBuf, String> = BTreeMap::new();
        for (key, source) in plan["assets"].as_object().cloned().unwrap_or_default() {
            let source = Path::new(source.as_str().ok_or("invalid export asset path")?);
            checked_file(source)?;
            let relative = format!(
                "assets/{key}.{}",
                source
                    .extension()
                    .and_then(|part| part.to_str())
                    .ok_or("asset extension missing")?
            );
            let canonical = source.canonicalize().map_err(|error| error.to_string())?;
            if let Some(existing) = asset_sources.get(&canonical) {
                card["assets"][&key] = json!(existing);
                continue;
            }
            asset_sources.insert(canonical, relative.clone());
            let destination = inside(&package, &relative)?;
            add_copy(source.to_path_buf(), destination)?;
            card["assets"][&key] = json!(relative);
        }
        let mut skills = Vec::new();
        for skill in plan["skills"].as_array().cloned().unwrap_or_default() {
            let name = sanitize_skill_name(skill["name"].as_str().ok_or("skill name missing")?)
                .ok_or("unsafe skill name")?;
            let relative = format!("skills/{name}");
            let source = Path::new(skill["path"].as_str().ok_or("skill path missing")?);
            for file in files_under(source)? {
                let relative_file = file
                    .strip_prefix(source)
                    .map_err(|error| error.to_string())?;
                let destination = inside(
                    &package,
                    &format!(
                        "{relative}/{}",
                        relative_file.to_str().ok_or("non-UTF8 skill path")?
                    ),
                )?;
                add_copy(file, destination)?;
            }
            skills.push(json!({"name": name, "path": relative}));
        }
        if !skills.is_empty() {
            card["skills"]["bundles"] = json!([{"name": format!("{} Bundle", plan["agent"]["name"].as_str().unwrap_or(id)), "skills": skills}]);
        }
        if body["exportMemory"] == true {
            card["memory"] =
                json!({"facts": plan["memoryFacts"], "compiled": plan["memoryCompiled"]});
        }
        let card_bytes = serde_json::to_vec_pretty(&card).map_err(|error| error.to_string())?;
        // 全部素材和完整描述累计通过后才创建副本，防止跨技能目录放大。
        charge(&mut budget, card_bytes.len() as u64)?;
        let mut copied = card_bytes.len() as u64;
        charge(&mut copied, 0)?;
        for (source, target) in copies {
            let bytes = bounded_file(&source)?;
            charge(&mut copied, bytes.len() as u64)?;
            std::fs::create_dir_all(target.parent().ok_or("material parent missing")?)
                .map_err(|error| error.to_string())?;
            std::fs::write(target, bytes).map_err(|error| error.to_string())?;
        }
        std::fs::write(package.join("card.json"), card_bytes).map_err(|error| error.to_string())?;
        let safe_name: String = plan["agent"]["name"]
            .as_str()
            .unwrap_or(id)
            .chars()
            .map(|character| {
                if character.is_alphanumeric() || matches!(character, '-' | '_') {
                    character
                } else {
                    '_'
                }
            })
            .take(80)
            .collect();
        let mut suffix = 0usize;
        let (path, output) = loop {
            let path = target.join(if suffix == 0 {
                format!("{safe_name}-charactercard.zip")
            } else {
                format!("{safe_name}-charactercard-{suffix}.zip")
            });
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => break (path, file),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => suffix += 1,
                Err(error) => return Err(error.to_string()),
            }
        };
        published = Some(path.clone());
        let mut writer = zip::ZipWriter::new(output);
        let mut total = 0u64;
        for source in files_under(&package)? {
            let bytes = std::fs::read(&source).map_err(|error| error.to_string())?;
            total += bytes.len() as u64;
            if total > MAX_PACKAGE_BYTES {
                return Err("export package exceeds size limit".into());
            }
            let relative = source
                .strip_prefix(&package)
                .map_err(|error| error.to_string())?
                .to_str()
                .ok_or("non-UTF8 archive path")?
                .replace('\\', "/");
            writer
                .start_file(
                    relative,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .map_err(|error| error.to_string())?;
            writer
                .write_all(&bytes)
                .map_err(|error| error.to_string())?;
        }
        writer
            .finish()
            .map_err(|error| error.to_string())?
            .sync_all()
            .map_err(|error| error.to_string())?;
        Ok::<_, String>(
            json!({"ok": true, "filePath": path, "fileName": path.file_name().and_then(|name| name.to_str()), "plan": preview(&plan, Some(id))}),
        )
    })();
    if let Err(cleanup) = std::fs::remove_dir_all(&stage) {
        return error(format!("export staging cleanup failed: {cleanup}"));
    }
    match result {
        Ok(value) => Json(value).into_response(),
        Err(message) => {
            if let Some(path) = published {
                if let Err(cleanup) = std::fs::remove_file(path) {
                    return error(format!("{message}; incomplete export cleanup: {cleanup}"));
                }
            }
            error(message)
        }
    }
}

fn validate_import_materials(plan: &Value, package: &Path) -> Result<(), String> {
    let mut total = serde_json::to_vec(plan)
        .map_err(|error| error.to_string())?
        .len() as u64;
    charge(&mut total, 0)?;
    let mut sources = HashSet::new();
    let mut names = HashSet::new();
    let mut count = 0;
    let mut consume = |file: &Path| -> Result<(), String> {
        count += 1;
        if count > MAX_ENTRIES {
            return Err("package contains too many material entries".into());
        }
        checked_file(file)?;
        if !sources.insert(file.canonicalize().map_err(|error| error.to_string())?) {
            return Err("duplicate import material source".into());
        }
        charge(
            &mut total,
            std::fs::metadata(file)
                .map_err(|error| error.to_string())?
                .len(),
        )
    };
    let mut assets = HashSet::new();
    for relative in plan["assets"]
        .as_object()
        .cloned()
        .unwrap_or_default()
        .values()
    {
        let file = inside(package, relative.as_str().ok_or("asset path missing")?)?;
        let canonical = file.canonicalize().map_err(|error| error.to_string())?;
        if assets.insert(canonical) {
            consume(&file)?;
        }
    }
    for skill in plan["skills"].as_array().cloned().unwrap_or_default() {
        if !names.insert(
            skill["name"]
                .as_str()
                .ok_or("skill name missing")?
                .to_string(),
        ) {
            return Err("duplicate skill install name".into());
        }
        let source = inside(
            package,
            skill["path"].as_str().ok_or("skill source missing")?,
        )?;
        for file in files_under(&source)? {
            consume(&file)?;
        }
    }
    Ok(())
}
pub async fn import_route(State(state): State<ServiceState>, Json(body): Json<Value>) -> Response {
    let Some(token) = body["token"].as_str() else {
        return bad("token is required");
    };
    // 上传、解包与规划在另一读写入口完成。无效确认先拒绝，不占用删除门。
    let (validated, validated_root) = match load_plan(&state, token) {
        Ok(value) => value,
        Err(message) => return bad(message),
    };
    if let Err(message) = validate_import_materials(&validated, &validated_root.join("package")) {
        return bad(message);
    }
    let _application = CARD_APPLICATION_GATE.lock().await;
    // 门内重读，重复确认不能拿门外旧计划再次创建助手。
    let (mut plan, root) = match load_plan(&state, token) {
        Ok(value) => value,
        Err(message) => return bad(message),
    };
    if let Some(id) = plan["createdAgentId"].as_str() {
        return fail(
            StatusCode::CONFLICT,
            format!(
                "character card already created agent {id}; inspect that agent before retrying"
            ),
        );
    }
    if let Err(message) = validate_import_materials(&plan, &root.join("package")) {
        return bad(message);
    }
    let name = plan["agent"]["name"].as_str().unwrap_or("");
    let yuan = plan["agent"]["yuan"].as_str().unwrap_or("lingxi");
    let preferred = plan["agent"]["id"].as_str().filter(|id| !id.is_empty());
    // 名称 slug 在大小写不敏感文件系统中可能碰到另一现役助手；导入始终分配明确新 ID。
    let chosen = preferred
        .map(|id| {
            if state.memory().agents_root().join(id).exists() {
                format!("{id}-{}", &token[..8])
            } else {
                id.into()
            }
        })
        .unwrap_or_else(|| format!("card-{}", &token[..16]));
    let (id, name) = match state.memory().create_agent(name, Some(&chosen), Some(yuan)) {
        Ok(value) => value,
        Err(message) => return error(message.message),
    };
    plan["createdAgentId"] = json!(id);
    if let Err(message) = save_plan(&root, &plan) {
        return error(format!(
            "agent {id} was created but import journal failed: {message}"
        ));
    }
    let result = async {
        if let Some(content) = plan["prompts"]["identity"].as_str().filter(|text| !text.is_empty()) {
            response_json(crate::persona::identity_put_route(State(state.clone()), RoutePath(id.clone()), Bytes::from(json!({"content": content}).to_string())).await).await?;
        }
        if let Some(content) = plan["prompts"]["agents"].as_str().filter(|text| !text.is_empty()) {
            response_json(crate::persona::agents_md_put_route(State(state.clone()), RoutePath(id.clone()), Bytes::from(json!({"content": content}).to_string())).await).await?;
        }
        if let Some(content) = plan["prompts"]["publicAgents"].as_str().filter(|text| !text.is_empty()) { response_json(crate::persona::public_agents_md_put_route(State(state.clone()), RoutePath(id.clone()), Bytes::from(json!({"content": content}).to_string())).await).await?; }
        if let Some(relative) = plan["assets"]["avatar"].as_str() {
            let path = inside(&root.join("package"), relative)?; checked_file(&path)?;
            let mime = image_mime(&path)?;
            let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
            let payload = json!({"dataUrl": format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))});
            response_json(crate::persona::avatar_post_route(State(state.clone()), RoutePath(id.clone()), Bytes::from(payload.to_string())).await).await?;
        }
        let mut installed = Vec::new();
        let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut prepared_bytes = 0;
        for (index, skill) in plan["skills"].as_array().cloned().unwrap_or_default().into_iter().enumerate() {
            let relative = skill["path"].as_str().ok_or("skill source missing")?;
            let source = inside(&root.join("package"), relative)?;
            let source_name = skill["name"].as_str().ok_or("skill name missing")?;
            let name = if state.skills().skill_exists(source_name) { format!("{source_name}-{}-{index}", &token[..8]) } else { source_name.to_string() };
            let prepared = root.join("prepared-skills").join(&name);
            copy_tree(&source, &prepared, &mut prepared_bytes)?;
            let text = std::fs::read_to_string(prepared.join("SKILL.md")).map_err(|error| error.to_string())?;
            am::atomic_write(&prepared.join("SKILL.md"), &sl::rewrite_skill_install_metadata(&text, &name, None)).map_err(|error| error.to_string())?;
            let skill = state.skills().install_manual_for_agent(&prepared, Some(&id)).map_err(|error| error.to_string())?;
            groups.entry(plan["skills"][index]["bundle"].as_str().unwrap_or("Imported Skills").into()).or_default().push(skill.name.clone());
            installed.push(json!({"name": skill.name, "originalName": source_name}));
        }
        let mut bundles = Vec::new();
        for (name, names) in groups { bundles.push(state.skills().create_imported_bundle(&name, &names, &id).map_err(|error| error.to_string())?); }
        let mut imported = 0;
        let mut imported_compiled = false;
        if body["importMemory"] == true {
            let facts = plan["memoryFacts"].as_array().cloned().unwrap_or_default();
            let tags = facts.iter().map(|fact| fact["tags"].as_array().cloned().unwrap_or_default().iter().filter_map(|tag| tag.as_str().map(str::to_string)).collect::<Vec<_>>()).collect::<Vec<_>>();
            let entries = facts.iter().enumerate().map(|(index, fact)| am::FactInsert { fact: fact["fact"].as_str().unwrap_or(""), tags: &tags[index], time: fact["time"].as_str(), session_id: fact["session_id"].as_str() }).collect::<Vec<_>>();
            imported = state.memory().fact_store(&id).map_err(|error| error.to_string())?.import_all(&entries).map_err(|error| error.to_string())?;
            let compiled = state.memory().compiled_store(&id).map_err(|error| error.to_string())?;
            let text = |key: &str| plan["memoryCompiled"][key].as_str().unwrap_or("").to_string();
            let sections = am::CompiledSections { facts: text("facts"), today: text("today"), week: text("week"), longterm: text("longterm") };
            imported_compiled = [&sections.facts, &sections.today, &sections.week, &sections.longterm].iter().any(|content| !content.is_empty());
            if imported_compiled { compiled.import_sections(&sections).map_err(|error| error.to_string())?; }
        }
        Ok::<_, String>(json!({"ok": true, "agent": {"id": id, "name": name}, "installedSkills": installed, "skillBundles": bundles, "importedMemory": imported, "importedCompiledMemory": imported_compiled, "plan": preview(&plan, None)}))
    }.await;
    match result {
        Ok(value) => {
            state
                .app_events()
                .publish("agent-created", json!({"id": id, "name": name}));
            if value["installedSkills"]
                .as_array()
                .is_some_and(|skills| !skills.is_empty())
            {
                state
                    .app_events()
                    .publish("skills-changed", json!({"agentId": id}));
            }
            Json(value).into_response()
        }
        Err(message) => error(format!(
            "agent {id} was created; character card import is incomplete: {message}"
        )),
    }
}
fn asset_response(path: &Path) -> Response {
    if let Err(message) = checked_file(path) {
        return fail(StatusCode::NOT_FOUND, message);
    }
    let mime = match image_mime(path) {
        Ok(value) => value,
        Err(message) => return bad(message),
    };
    match std::fs::read(path) {
        Ok(bytes) => ([(header::CONTENT_TYPE, mime)], bytes).into_response(),
        Err(message) => error(message.to_string()),
    }
}
pub async fn plan_asset_route(
    State(state): State<ServiceState>,
    RoutePath((token, key)): RoutePath<(String, String)>,
) -> Response {
    let (plan, root) = match load_plan(&state, &token) {
        Ok(value) => value,
        Err(message) => return bad(message),
    };
    let Some(relative) = plan["assets"][&key].as_str() else {
        return fail(StatusCode::NOT_FOUND, "asset not found");
    };
    match inside(&root.join("package"), relative) {
        Ok(path) => asset_response(&path),
        Err(message) => bad(message),
    }
}
pub async fn export_asset_route(
    State(state): State<ServiceState>,
    RoutePath((id, key)): RoutePath<(String, String)>,
) -> Response {
    let config = match response_json(
        crate::agents_admin::agent_config_readback::agent_config_get_route(
            State(state.clone()),
            RoutePath(id.clone()),
        )
        .await,
    )
    .await
    {
        Ok(value) => value,
        Err(message) => return error(message),
    };
    let assets = match asset_sources(
        &state,
        &id,
        config["agent"]["yuan"].as_str().unwrap_or("lingxi"),
    )
    .await
    {
        Ok(value) => value,
        Err(message) => return error(message),
    };
    match assets[&key].as_str() {
        Some(path) => asset_response(Path::new(path)),
        None => fail(StatusCode::NOT_FOUND, "asset not found"),
    }
}
