"""R00-T02 的源码反向门禁：分支选择和 UI 消费链不能由候选摘要自证。"""

from __future__ import annotations

import re
import hashlib
import json
import subprocess
from pathlib import Path
from typing import Callable


IMPORT_RE = re.compile(r"(?:\bfrom\s*|\bimport\s*\(|\brequire\s*\()\s*['\"]([^'\"]+)['\"]")
SOURCE_EXTENSIONS = (".tsx", ".ts", ".jsx", ".js", ".cjs", ".mjs")
SourceReader = Callable[[str], list[str]]


# 这些边由现役控件的条件和最终调用点反向列出，不从候选矩阵求并集。
# 同页其他动作已挂某叶，不代表这里的控件也已挂对。
SOURCE_REQUIRED_UI_EDGES = [
    ("ui:panel:automation#01", "semantic-effect:desk.desk_cron.read", "desktop/src/react/components/AutomationPanel.tsx", "lingxiFetch('/api/desk/cron'"),
    ("ui:panel:automation#01", "semantic-effect:models:/models:查看可用模型", "desktop/src/react/components/AutomationPanel.tsx", "lingxiFetch('/api/models')"),
    ("ui:panel:automation#02", "semantic-effect:cron.add", "desktop/src/react/components/AutomationPanel.tsx", "action: 'add'"),
    ("ui:panel:automation#04", "semantic-effect:cron.remove", "desktop/src/react/components/AutomationPanel.tsx", "action: 'remove'"),
    ("ui:panel:automation#05", "semantic-effect:cron.update", "desktop/src/react/components/AutomationPanel.tsx", "action: 'update'"),
    ("ui:settings:agent#01", "semantic-effect:agents./agents/.id/config.read", "desktop/src/react/settings/actions.ts", "lingxiFetch(`${agentBase}/config`"),
    ("ui:settings:agent#01", "semantic-effect:agents./agents/.id/identity.read", "desktop/src/react/settings/actions.ts", "lingxiFetch(`${agentBase}/identity`"),
    ("ui:settings:agent#01", "semantic-effect:agents./agents/.id/agents-md.read", "desktop/src/react/settings/actions.ts", "lingxiFetch(`${agentBase}/agents-md`"),
    ("ui:settings:agent#01", "semantic-effect:agents./agents/.id/public-agents-md.read", "desktop/src/react/settings/actions.ts", "lingxiFetch(`${agentBase}/public-agents-md`"),
    ("ui:settings:agent#01", "semantic-effect:config.user_profile.read", "desktop/src/react/settings/actions.ts", "lingxiFetch('/api/user-profile'"),
    ("ui:settings:agent#01", "semantic-effect:preferences./preferences/models.read", "desktop/src/react/settings/actions.ts", "lingxiFetch('/api/preferences/models'"),
    ("ui:settings:agent#01", "semantic-effect:agents./agents/.id/experience.read", "desktop/src/react/settings/actions.ts", "lingxiFetch(`${agentBase}/experience`"),
    ("ui:settings:agent#01", "semantic-effect:agents./agents/switch.post", "desktop/src/react/settings/actions.ts", "'/api/agents/switch'"),
    ("ui:settings:agent#01", "semantic-effect:character-cards./character-cards/plan.post", "desktop/src/react/settings/overlays/AgentCreateOverlay.tsx", "'/api/character-cards/plan'"),
    ("ui:settings:agent#01", "behavior:character-cards:/character-cards/import:POST", "desktop/src/react/settings/overlays/AgentCreateOverlay.tsx", "'/api/character-cards/import'"),
    ("ui:settings:agent#01", "semantic-effect:agents./agents/.id/skills/cleanup-preview.read", "desktop/src/react/settings/overlays/AgentDeleteOverlay.tsx", "/skills/cleanup-preview"),
    ("ui:settings:agent#01", "semantic-effect:character-cards./character-cards/plans/.token/assets/.asset.read", "desktop/src/react/settings/overlays/CharacterCardPreviewOverlay.tsx", "/api/character-cards/plans/${plan.token}/assets/${key}"),
    ("ui:settings:agent#06", "semantic-effect:character-cards./character-cards/export/.agentid/assets/.asset.read", "desktop/src/react/settings/overlays/CharacterCardPreviewOverlay.tsx", "/api/character-cards/export/${encodeURIComponent(plan.agentId)}/assets/${key}"),
    ("ui:settings:agent#01", "semantic-effect:agents./agents/.id/avatar.read", "desktop/src/react/settings/tabs/agent/AgentCardStack.tsx", "agent.hasAvatar"),
    ("ui:settings:agent#11", "semantic-effect:agents./agents/.id/avatar.read", "desktop/src/react/settings/tabs/agent/AgentCardStack.tsx", "agent.hasAvatar"),
    ("ui:settings:agent#14", "semantic-effect:preferences./preferences/models.read", "desktop/src/react/settings/tabs/agent/AgentMemoryEmbedding.tsx", "fetchJson('/api/preferences/models')"),
    ("ui:settings:agent#14", "semantic-effect:agents./agents/.id/config.read", "desktop/src/react/settings/tabs/agent/AgentMemoryEmbedding.tsx", "fetchJson(`/api/agents/${encodeURIComponent(agentId)}/config`)"),
    ("ui:settings:agent#02", "semantic-effect:agents./agents/primary.put", "desktop/src/react/settings/actions.ts", "'/api/agents/primary'"),
    ("ui:settings:agent#03", "semantic-effect:agents./agents/.id/identity.put", "desktop/src/react/settings/tabs/AgentTab.tsx", "${agentBase}/identity"),
    ("ui:settings:agent#03", "semantic-effect:agents./agents/.id/agents-md.put", "desktop/src/react/settings/tabs/AgentTab.tsx", "${agentBase}/agents-md"),
    ("ui:settings:agent#05", "semantic-effect:agents./agents/.id/experience.put", "desktop/src/react/settings/tabs/agent/AgentExperience.tsx", "/experience"),
    ("ui:settings:agent#19", "semantic-effect:agents./agents/order.put", "desktop/src/react/settings/tabs/agent/AgentCardStack.tsx", "'/api/agents/order'"),
    ("ui:settings:access#03", "semantic-effect:access.access_summary.read", "desktop/src/react/settings/tabs/AccessTab.tsx", "await loadSummary()"),
    ("ui:settings:access#05", "semantic-effect:access.access_summary.read", "desktop/src/react/settings/tabs/AccessTab.tsx", "await loadSummary()"),
    ("ui:settings:providers#06", "semantic-effect:provider.global.save", "desktop/src/react/settings/tabs/providers/ApiKeyCredentials.tsx", "saveProviderConfigPatch(providerId, payload)"),
    ("ui:settings:providers#04", "semantic-effect:provider.global.save", "desktop/src/react/settings/tabs/providers/ApiKeyCredentials.tsx", "saveProviderConfigPatch(providerId, { base_url: trimmed })"),
    ("ui:settings:providers#04", "semantic-effect:provider.global.save", "desktop/src/react/settings/tabs/providers/ApiKeyCredentials.tsx", "saveProviderConfigPatch(providerId, { api: val })"),
    ("ui:settings:providers#05", "semantic-effect:providers.providers_name_discovered_models.read", "desktop/src/react/settings/tabs/providers/ProviderModelList.tsx", "loadDiscoveredModels();"),
    ("ui:settings:providers#03", "ui:settings:providers", "desktop/src/react/settings/tabs/providers/ProviderDetail.tsx", "onRemoveDraft ?"),
    ("ui:settings:providers#06", "semantic-effect:providers.providers_test.post", "desktop/src/react/settings/tabs/providers/ApiKeyCredentials.tsx", "'/api/providers/test'"),
    ("ui:settings:providers#14", "semantic-effect:providers.providers_name_api_key.read", "desktop/src/react/settings/tabs/providers/ApiKeyCredentials.tsx", "revealSavedApiKey"),
    ("ui:settings:me#02", "semantic-effect:avatar:/avatar/:role:上传指定用户或 Agent 的头像", "desktop/src/react/settings/overlays/CropOverlay.tsx", "uploadCroppedAvatar"),
    ("ui:settings:general#02", "semantic-effect:agent.config.global_update", "desktop/src/react/settings/tabs/GeneralTab.tsx", "keep_awake: on"),
    ("ui:settings:experiments#02", "desktop-behavior:settings-changed", "desktop/src/react/settings/tabs/ExperimentsTab.tsx", "settingsChanged?.('experiment-changed'"),
    ("ui:settings:general#03", "desktop-behavior:settings-changed", "desktop/src/react/settings/tabs/GeneralTab.tsx", "settingsChanged?.(options.eventName"),
    ("ui:settings:keybindings#02", "desktop-behavior:settings-changed", "desktop/src/react/settings/tabs/KeybindingsTab.tsx", "settingsChanged?.('keybindings-changed'"),
    ("ui:settings:keybindings#03", "desktop-behavior:settings-changed", "desktop/src/react/settings/tabs/KeybindingsTab.tsx", "settingsChanged?.('keybindings-changed'"),
    ("ui:settings:keybindings#03", "desktop-behavior:global-shortcut", "desktop/src/react/settings/tabs/KeybindingsTab.tsx", "keybindingsReloadGlobal?.()"),
    ("ui:settings:interface#02", "desktop-behavior:settings-changed", "desktop/src/react/settings/tabs/InterfaceTab.tsx", "settingsChanged?.('theme-changed'"),
    ("ui:settings:interface#03", "desktop-behavior:settings-changed", "desktop/src/react/settings/tabs/InterfaceTab.tsx", "settingsChanged?.('editor-typography-changed'"),
    ("ui:settings:interface#04", "desktop-behavior:settings-changed", "desktop/src/react/settings/tabs/InterfaceTab.tsx", "settingsChanged?.('hardware-acceleration-changed'"),
    ("ui:settings:interface#05", "desktop-behavior:settings-changed", "desktop/src/react/settings/tabs/InterfaceTab.tsx", "settingsChanged?.('sidebar-ui-changed'"),
    ("ui:settings:security#03", "desktop-behavior:settings-changed", "desktop/src/react/settings/tabs/SecurityTab.tsx", "settingsChanged?.('network-proxy-changed'"),
]

# 源码反查已证实这些控件不调用所列效果；防止页面并集重新把死边算作命中。
SOURCE_FORBIDDEN_UI_EDGES = [
    ("ui:settings:agent#01", "semantic-effect:settings-snapshot.settings_snapshot.read"),
    ("ui:settings:work#01", "semantic-effect:config.config.read"),
    ("ui:settings:me#01", "semantic-effect:config.config.read"),
    ("ui:settings:me#01", "semantic-effect:config.user_profile.read"),
    ("ui:settings:interface#01", "semantic-effect:preferences./preferences/appearance.read"),
    ("ui:settings:experiments#04", "behavior:preferences:/preferences/computer-use/approvals:POST"),
    ("ui:settings:experiments#04", "behavior:preferences:/preferences/computer-use/approvals:DELETE"),
]

_SETTINGS_CONFIG_FANOUT = (
    "semantic-effect:agents./agents/.id/config.read",
    "semantic-effect:agents./agents/.id/identity.read",
    "semantic-effect:agents./agents/.id/agents-md.read",
    "semantic-effect:agents./agents/.id/public-agents-md.read",
    "semantic-effect:config.user_profile.read",
    "semantic-effect:preferences./preferences/models.read",
    "semantic-effect:agents./agents/.id/experience.read",
)
for _agent_action in ("ui:settings:agent#02", "ui:settings:agent#03"):
    SOURCE_REQUIRED_UI_EDGES.extend(
        (_agent_action, _effect, "desktop/src/react/settings/tabs/AgentTab.tsx", "await loadSettingsConfig()")
        for _effect in _SETTINGS_CONFIG_FANOUT
    )
    SOURCE_REQUIRED_UI_EDGES.append(
        (_agent_action, "semantic-effect:agents./agents.read", "desktop/src/react/settings/tabs/AgentTab.tsx", "await loadAgents()")
    )
for _provider_action, _source, _marker in (
    ("ui:settings:providers#02", "desktop/src/react/settings/tabs/providers/ProviderList.tsx", "await loadSettingsConfig()"),
    ("ui:settings:providers#03", "desktop/src/react/settings/tabs/providers/ProviderDetail.tsx", "await onRefresh()"),
    ("ui:settings:providers#06", "desktop/src/react/settings/tabs/providers/ApiKeyCredentials.tsx", "if (!await refreshAfterSave()) return;"),
):
    SOURCE_REQUIRED_UI_EDGES.extend(
        (_provider_action, _effect, _source, _marker) for _effect in _SETTINGS_CONFIG_FANOUT
    )
    SOURCE_REQUIRED_UI_EDGES.append(
        (_provider_action, "semantic-effect:providers.providers_summary.read", _source, _marker)
    )
SOURCE_REQUIRED_UI_EDGES.extend(
    ("ui:settings:agent#05", _effect, "desktop/src/react/settings/tabs/AgentTab.tsx", "if (saved) await loadSettingsConfig();")
    for _effect in _SETTINGS_CONFIG_FANOUT
)
SOURCE_REQUIRED_UI_EDGES.append(
    ("ui:settings:agent#01", "semantic-effect:agents./agents/.id/config.read", "desktop/src/react/settings/overlays/AgentDeleteOverlay.tsx", "await loadSettingsConfig();")
)


# 每条记录是一处独立的请求体；同一动作可以有多个控件和多次请求。
# 字段归属以 CONFIG_SCHEMA/splitByScope 为准，URL 中的助手 ID 只决定传输地址。
SOURCE_AUTOSAVE_CONFIG_REQUESTS = [
    ("ui:settings:agent#02", "desktop/src/react/settings/tabs/AgentTab.tsx", "autoSaveConfig({ models:", ("models.chat",)),
    ("ui:settings:agent#04", "desktop/src/react/settings/tabs/agent/AgentMemory.tsx", "autoSaveConfig({ memory:", ("memory.enabled",)),
    ("ui:settings:agent#05", "desktop/src/react/settings/tabs/AgentTab.tsx", "autoSaveConfig({ experience:", ("experience.enabled",)),
    ("ui:settings:agent#08", "desktop/src/react/settings/tabs/agent/AgentToolsSection.tsx", "autoSaveConfig({ tools:", ("tools.disabled",)),
    ("ui:settings:general#02", "desktop/src/react/settings/tabs/GeneralTab.tsx", "autoSaveConfig({ keep_awake:", ("keep_awake",)),
    ("ui:settings:interface#03", "desktop/src/react/settings/tabs/InterfaceTab.tsx", "autoSaveConfig({ editor:", ("editor",)),
    ("ui:settings:interface#03", "desktop/src/react/settings/tabs/InterfaceTab.tsx", "autoSaveConfig({ chat:", ("chat",)),
    ("ui:settings:interface#04", "desktop/src/react/settings/tabs/InterfaceTab.tsx", "autoSaveConfig({ hardware_acceleration:", ("hardware_acceleration",)),
    ("ui:settings:interface#06", "desktop/src/react/settings/tabs/InterfaceTab.tsx", "autoSaveConfig({ locale:", ("locale",)),
    ("ui:settings:interface#06", "desktop/src/react/settings/tabs/InterfaceTab.tsx", "autoSaveConfig({ timezone:", ("timezone",)),
    ("ui:settings:security#01", "desktop/src/react/settings/tabs/SecurityTab.tsx", "autoSaveConfig({ sandbox:", ("sandbox",)),
    ("ui:settings:security#01", "desktop/src/react/settings/tabs/SecurityTab.tsx", "autoSaveConfig({ sandbox_network:", ("sandbox_network",)),
    ("ui:settings:security#02", "desktop/src/react/settings/tabs/SecurityTab.tsx", "autoSaveConfig({ file_backup:", ("file_backup",)),
    ("ui:settings:security#03", "desktop/src/react/settings/tabs/SecurityTab.tsx", "autoSaveConfig({ network_proxy:", ("network_proxy",)),
    ("ui:settings:work#02", "desktop/src/react/settings/tabs/WorkTab.tsx", "autoSaveConfig({ desk:", ("desk.heartbeat_master",)),
    ("ui:settings:work#02", "desktop/src/react/settings/tabs/WorkTab.tsx", "autoSaveConfig({ automation:", ("automation.permissionMode",)),
    ("ui:settings:skills#07", "desktop/src/react/settings/tabs/skills/SkillCapabilities.tsx", "autoSaveConfig(", ("capabilities.learn_skills",)),
]


_PROVIDER_MODEL_PROJECTION = "models.json：仅投影变化、文件缺失或权限修正时写入；不是供应商权威存储"
_PROVIDER_FOCUS_YAML = "焦点助手 config.yaml：onProviderChanged 后无 agentId 的 updateConfig({}) 在焦点存在时重写"
_PROVIDER_CATALOG = "$LINGXI_HOME/provider-catalog.json：供应商用户条目的权威存储；删除不存在条目时可不改动"
_PROVIDER_PLUGIN = "$LINGXI_HOME/provider-plugins/<storageId>/manifest.json 与 providers/<storageId>.json：仅本地自定义或未注册插件保存时写入，删除本地插件时删其目录；内置声明不删除"


def provider_leaf_expected(key: str) -> dict:
    """以源码条件定义五条供应商叶，正源、库存和场景必须逐字段一致。"""
    agent = key.startswith("provider.agent.")
    inline = key == "provider.inline_credential.save"
    remove = key.endswith(".remove")
    if key not in {"provider.agent.save", "provider.agent.remove", "provider.inline_credential.save",
                   "provider.global.save", "provider.global.remove"}:
        raise ValueError(f"未知供应商叶: {key}")
    contract = {
        "catalog": "save_after_provider_resolution" if inline else "remove_if_user_entry_or_local_plugin_exists" if remove else "save_user_entry",
        "local_plugin": "remove_only_if_local_plugin" if remove else "write_only_if_local_or_unregistered",
        "models_projection": "write_only_if_changed_missing_or_permission_fix",
        "focus_yaml": "rewrite_after_model_refresh_if_focus_exists_without_agent_id",
        "request_yaml": "clear_inline_secret_then_explicit_save_requested_agent" if inline else "no_explicit_save_for_pure_providers" if agent else "not_applicable",
        "preferences": "only_if_separate_global_fields_present",
        "failure": "earlier_catalog_or_plugin_write_may_survive_later_error",
    }
    stores = [_PROVIDER_CATALOG, _PROVIDER_PLUGIN, _PROVIDER_MODEL_PROJECTION,
              _PROVIDER_FOCUS_YAML + ("；焦点可与请求 :id 不同" if agent or inline else "；/api/config 没有请求助手 ID")]
    if inline:
        stores.append("请求助手 agents/{id}/config.yaml：凭证清空后 api/embedding_api 块仍进入显式 saveConfig；后续运行时刷新可能回退到焦点助手")
    action = "删除" if remove else "转存内联凭证" if inline else "保存"
    route = "PUT /api/agents/:id/config" if agent or inline else "PUT /api/config"
    result = (
        f"{route} {action}供应商用户条目；权威数据在 provider-catalog.json。"
        "本地插件文件与 models.json 投影按条件变化；成功刷新时无 agentId 的 updateConfig({}) 可重写焦点助手 YAML。"
        + ("内联秘密清空后还显式保存请求助手 YAML。" if inline else
           "纯 providers 请求不进入请求助手的显式 YAML 保存分支。" if agent else
           "仅同一请求另含全局字段时才写 preferences.json。")
    )
    assertions = [
        "准备请求助手 A、焦点助手 B（A≠B）转存 api/embedding_api 内联凭证：先读 provider-catalog.json，再核对 B 的刷新 YAML 与 A 的显式 YAML；A 内联秘密须清空且不得保留明文。" if inline else
        f"准备请求助手 A、焦点助手 B（A≠B）并执行 {route} {action}：读回 provider-catalog.json；纯供应商成功刷新可重写 B 的 config.yaml，但不经请求 A 的显式保存；A=B 时按同一焦点路径核对。" if agent else
        f"在焦点助手存在时执行 {route} {action}：读回 provider-catalog.json 与焦点 config.yaml；该路由没有请求助手 ID，纯供应商请求不写 preferences.json。",
        "仅本地自定义或未注册插件保存才写 provider-plugins；仅删除本地插件才删文件。内置覆盖项删除后内置声明仍在；不存在的条目删除可不改目录。" if remove else
        "仅本地自定义或未注册插件保存才写 provider-plugins；内置供应商不因此写插件文件。",
        "models.json 只是派生投影，内容不变且权限正确可不重写；供应商用户数据以 provider-catalog.json 为准。",
        "onProviderChanged 或后续焦点刷新失败时，先前目录/插件文件可能已变而 HTTP 报错；无焦点时无 ID 刷新可失败，不宣称原子回滚，并逐处读回。",
        "同请求多供应商项逐项处理，后项失败可保留前项；只有另含全局字段才写 preferences.json，不写人格或记忆文件。",
    ]
    if inline:
        assertions.append("provider 无法解析时先返回 400；成功转存后仍须检查请求助手 YAML 的清空字段，后段失败不可把目录中已存凭证当作回滚。")
        assertions.append("若请求助手 A 的运行时实例未加载，显式保存 A 后的 updateConfig({agentId:A}) 仍可能回退到焦点 B，分别核对磁盘和运行时。")
    return {
        "storage_contract": contract, "current_stores": stores, "result": result,
        "assertions": assertions,
        "acceptance_scope": "具备设置、供应商与秘密权限；准备已注册内置供应商、一个本地自定义供应商、请求助手 A 与焦点助手 B，并保留目录及 YAML 的写入前快照" if agent or inline else
                            "具备设置、供应商与秘密权限；准备已注册内置供应商、一个本地自定义供应商和焦点助手，并保留目录及 YAML 的写入前快照",
        "acceptance_when": f"通过 {route} 分别执行纯 {action}、A=B、A≠B、本地插件、多供应商项和刷新失败；内联凭证另测缺 provider" if agent or inline else
                           f"通过 {route} 分别执行纯 {action}、与全局字段同请求、本地插件、多供应商项和刷新失败",
        "acceptance_then": "逐处读回权威目录、条件插件文件、条件模型投影、焦点 YAML、请求助手 YAML 与仅另含全局字段才变化的 preferences；失败后记录可能的部分成功，不把 HTTP 错误当回滚" if agent or inline else
                           "逐处读回权威目录、条件插件文件、条件模型投影、焦点 YAML 与仅另含全局字段才变化的 preferences；失败后记录可能的部分成功，不把 HTTP 错误当回滚",
    }


def classify_config_patch(patch: dict, global_fields: set[str], transport: str = "PUT /api/agents/:id/config") -> set[str]:
    """镜像 splitByScope 的两级字段拆分及 agents PUT 的供应商特殊分流。"""
    if transport == "PUT /api/config":
        effects = {"semantic-effect:config.global.update"} if any(key != "providers" for key in patch) else set()
        for item in patch.get("providers", {}).values():
            effects.add("semantic-effect:provider.global.remove" if item is None else "semantic-effect:provider.global.save")
        return effects
    if transport != "PUT /api/agents/:id/config":
        raise ValueError(f"未知配置写入路由: {transport}")
    effects: set[str] = set()
    agent_remaining = False
    for key, value in patch.items():
        if key in global_fields:
            effects.add("semantic-effect:agent.config.global_update")
            continue
        if key == "providers" and isinstance(value, dict):
            effects.update("semantic-effect:provider.agent.remove" if item is None else "semantic-effect:provider.agent.save"
                           for item in value.values())
            continue
        if key in {"api", "embedding_api"} and isinstance(value, dict):
            if any(name in value for name in ("api_key", "base_url")):
                effects.add("semantic-effect:provider.inline_credential.save")
                # clearInlineProviderCredentialFields 写空值而非删除 block；
                # agentPartial 仍非空，后续 saveConfig 也会执行。
                agent_remaining = True
            if set(value) - {"api_key", "base_url"}:
                agent_remaining = True
            continue
        if isinstance(value, dict):
            for child in value:
                path = f"{key}.{child}"
                if path in global_fields:
                    effects.add("semantic-effect:agent.config.global_update")
                else:
                    agent_remaining = True
        else:
            agent_remaining = True
    if agent_remaining:
        effects.add("semantic-effect:agent.config.update")
    return effects


def validate_config_request_cases(cases: list[dict], global_fields: set[str]) -> None:
    required = {"pure_global", "pure_agent", "mixed", "provider_save", "provider_remove", "inline_credential",
                "other_route_provider", "other_route_remove", "other_route_mixed", "provider_multi",
                "provider_local_save", "provider_local_remove", "provider_no_focus", "provider_refresh_failure",
                "provider_same_focus"}
    if {case.get("id") for case in cases} != required or len(cases) != len(required):
        raise ValueError("配置分流正反请求样例缺项")
    for case in cases:
        actual = classify_config_patch(case["patch"], global_fields, case["transport"])
        if set(case.get("effect_entry_ids", [])) != actual:
            raise ValueError(f"配置请求体与写入叶不符: {case['id']}")
        if case["id"] == "mixed":
            boundary = case.get("failure_boundary", "")
            if not all(word in boundary for word in ("先写全局", "后段失败", "可能保留前段写入", "不得宣称原子回滚")):
                raise ValueError("mixed patch 错误描述为原子回滚或遗漏前段写入")
        if actual & {f"semantic-effect:{key}" for key in (
            "provider.agent.save", "provider.agent.remove", "provider.inline_credential.save",
            "provider.global.save", "provider.global.remove"
        )}:
            if case.get("patch") != provider_case_patch(case["id"]):
                raise ValueError(f"供应商请求样例输入条件错挂: {case['id']}")
            if case.get("storage_expectation") != provider_case_expected(case["id"]):
                raise ValueError(f"供应商请求样例存储或失败边界错挂: {case['id']}")
            if case.get("setup") != provider_case_setup(case["id"]):
                raise ValueError(f"供应商请求样例焦点或前置状态错挂: {case['id']}")
            if case.get("failure_boundary") != provider_case_boundary(case["id"]):
                raise ValueError(f"供应商请求样例失败文字错挂: {case['id']}")


def provider_case_expected(case_id: str) -> dict:
    """场景条件是单独的结构化事实，不以 F-ID 正确推断写盘正确。"""
    focus = "B" if case_id not in {"provider_same_focus", "provider_no_focus", "other_route_provider", "other_route_remove", "other_route_mixed"} else (
        "A" if case_id == "provider_same_focus" else "none" if case_id == "provider_no_focus" else "current")
    return {
        "catalog": "may_remain_unchanged_if_entry_absent" if case_id in {"provider_remove", "other_route_remove"} else
                   "writes_deletion_marker_if_local_exists" if case_id == "provider_local_remove" else "may_change_before_refresh",
        "plugin": "conditional_local_only",
        "models_json": "conditional_derived_projection",
        "focus_yaml": "not_reached_if_provider_refresh_fails" if case_id == "provider_refresh_failure" else
                      "fails_without_focus_after_catalog" if case_id == "provider_no_focus" else f"rewrite_{focus}_after_refresh",
        "requested_yaml": "explicit_clear_inline_then_save_A" if case_id == "inline_credential" else "no_explicit_save_A_for_pure_providers" if not case_id.startswith("other_route") else "not_applicable",
        "preferences": "only_if_global_fields" if case_id == "other_route_mixed" else "unchanged_for_pure_providers",
        "failure": "prior_writes_may_survive_error_no_atomic_rollback",
    }


def provider_case_boundary(case_id: str) -> str:
    common = "provider-catalog.json 可先变；provider-plugins 与 models.json 仅条件变化；刷新失败不原子回滚。"
    details = {
        "provider_save": "请求 A、焦点 B：纯 providers 不走 A 的显式 YAML 保存；成功刷新仍可重写 B 的 YAML。",
        "provider_remove": "已有覆盖项可删目录记录；不存在项可不改目录；内置声明保留，成功刷新仍可重写焦点 B 的 YAML。",
        "inline_credential": "内联凭证转存目录并清空后，还会显式写请求 A 的 YAML；此前可重写焦点 B；provider 无法解析先返回 400。",
        "other_route_provider": "PUT /api/config 纯供应商保存不自动写 preferences；成功刷新可重写焦点 YAML。",
        "other_route_remove": "PUT /api/config 的 null 项归删除叶；不存在项可不改目录，仍尝试刷新焦点 YAML。",
        "other_route_mixed": "PUT /api/config 另含全局字段才写 preferences；全局 setter 或目录先成功、后段失败可部分生效。",
        "provider_multi": "同请求多供应商项逐项处理，后项失败可留下前项目录写入。",
        "provider_local_save": "本地自定义供应商才条件写 provider-plugins；内置供应商不会因此写插件文件。",
        "provider_local_remove": "仅本地插件条件删除插件文件；内置声明不删除，条目不存在可提前返回。",
        "provider_no_focus": "目录及模型投影可能已变，但无焦点时 updateConfig({}) 可失败，不能声称 YAML 已刷新。",
        "provider_refresh_failure": "onProviderChanged 失败时目录可能已变，后续 updateConfig({}) 不执行；models.json 是否已写取决于失败位置。",
        "provider_same_focus": "请求 A 与焦点 A 相同，纯 providers 不走显式分支，但成功刷新可重写 A 的 YAML。",
    }
    return common + details[case_id]


def provider_case_setup(case_id: str) -> dict:
    conditions = {
        "provider_save": ("A", "B", "内置 openai"),
        "provider_remove": ("A", "B", "已有 openai 用户覆盖项；另测不存在项"),
        "inline_credential": ("A", "B", "请求助手 A 的 api 已可解析"),
        "other_route_provider": (None, "current", "内置 openai"),
        "other_route_remove": (None, "current", "已有 openai 用户覆盖项；另测不存在项"),
        "other_route_mixed": (None, "current", "另含全局字段"),
        "provider_multi": ("A", "B", "多供应商项，第二项可注入失败"),
        "provider_local_save": ("A", "B", "未注册的本地自定义供应商"),
        "provider_local_remove": ("A", "B", "已有本地插件"),
        "provider_no_focus": ("A", None, "无焦点助手"),
        "provider_refresh_failure": ("A", "B", "onProviderChanged 注入失败"),
        "provider_same_focus": ("A", "A", "请求与焦点同为 A"),
    }
    request_agent, focus_agent, condition = conditions[case_id]
    return {"request_agent": request_agent, "focus_agent": focus_agent, "condition": condition}


def provider_case_patch(case_id: str) -> dict:
    saved = {"providers": {"openai": {"api_key": "test"}}}
    removed = {"providers": {"openai": None}}
    patches = {
        "provider_save": saved, "provider_remove": removed,
        "inline_credential": {"api": {"provider": "openai", "api_key": "test"}},
        "other_route_provider": saved, "other_route_remove": removed,
        "other_route_mixed": {"keep_awake": True, **saved},
        "provider_multi": {"providers": {"openai": {"api_key": "test"}, "anthropic": None}},
        "provider_local_save": {"providers": {"r00-local-provider": {"api": "openai-completions", "base_url": "https://example.invalid/v1"}}},
        "provider_local_remove": {"providers": {"r00-local-provider": None}},
        "provider_no_focus": saved, "provider_refresh_failure": saved, "provider_same_focus": saved,
    }
    return patches[case_id]


def validate_provider_leaf_contracts(by_entry: dict[str, dict], reviews: dict[str, dict], root: Path,
                                     reader: SourceReader | None = None) -> None:
    """同时核对现役调用链、五叶正源及生成库存，不让正确 F-ID 遮盖错误存储。"""
    anchors = {
        "server/routes/agents.ts": ("engine.providerRegistry.saveProvider", "engine.providerRegistry.removeProvider",
                                    "await engine.onProviderChanged()", "await engine.updateConfig({})",
                                    "delete agentPartial.providers", "Object.keys(agentPartial).length === 0",
                                    "clearInlineProviderCredentialFields(block)", "saveConfig(configPath, agentPartial)"),
        "server/routes/config.ts": ("engine.providerRegistry.saveProvider", "engine.providerRegistry.removeProvider",
                                    "await engine.onProviderChanged()", "await engine.updateConfig({})",
                                    "agentOwnedKeys.length > 0", "engine[setter](value)"),
        "core/provider-registry.ts": ("this._catalog.saveProviders", "persistAsLocalPlugin", "this._localProviderPlugins.removeProvider",
                                      "if (!hasCatalogEntry && !hasLocalPlugin) return;"),
        "core/provider-catalog.ts": ("provider-catalog.json", "saveProviders("),
        "core/local-provider-plugin-store.ts": ("writeProvider(", "removeProvider("),
        "core/engine.ts": ("async onProviderChanged()", "this._models.reloadAndSync()"),
        "core/model-manager.ts": ("async syncAndRefresh()", "syncModels(projection.providers"),
        "core/model-sync.ts": ("if (oldStr === newStr)", "writeSecretFileSync(modelsJsonPath, newStr)"),
        "core/config-coordinator.ts": ("this._d.getAgentById?.(agentId)", "this._d.getAgent()", "agent.updateConfig(partial)"),
        "core/agent.ts": ("updateConfig(partial, options", "saveConfig(this.configPath, partial)"),
        "lib/memory/config-loader.ts": ("export function saveConfig(configPath, partial)", "writeSecretFileSync(configPath, yamlStr)"),
    }
    for source, markers in anchors.items():
        content = _source_text(root / source, root, reader)
        if any(marker not in content for marker in markers):
            raise ValueError(f"供应商存储调用链源码锚点变化: {source}")
    agent_route = _source_text(root / "server/routes/agents.ts", root, reader)
    global_route = _source_text(root / "server/routes/config.ts", root, reader)
    if not (agent_route.index("engine.providerRegistry.saveProvider") < agent_route.index("await engine.onProviderChanged()")
            < agent_route.index("await engine.updateConfig({})") < agent_route.index("Object.keys(agentPartial).length === 0")
            < agent_route.index("saveConfig(configPath, agentPartial)")):
        raise ValueError("供应商目录、焦点刷新及请求助手显式保存的顺序变化")
    if not (global_route.index("engine[setter](value)") < global_route.index("engine.providerRegistry.saveProvider")
            < global_route.index("await engine.onProviderChanged()") < global_route.index("await engine.updateConfig({})")):
        raise ValueError("全局配置 setter、供应商目录及焦点刷新的顺序变化")
    if not re.search(r"if \(refreshDescription\) agent\.updateConfig\(partial, \{ refreshDescription: true \}\);\s*else agent\.updateConfig\(partial\);",
                     _source_text(root / "core/config-coordinator.ts", root, reader)):
        raise ValueError("空对象刷新不再落到 Agent.updateConfig")
    for entry_id, review in reviews.items():
        if entry_id not in {"behavior:agents:/agents/:id/config:PUT", "behavior:config:/config:PUT"}:
            continue
        for leaf in review["leaves"]:
            key = leaf["semantic_key"]
            if key not in {"provider.agent.save", "provider.agent.remove", "provider.inline_credential.save",
                           "provider.global.save", "provider.global.remove"}:
                continue
            expected = provider_leaf_expected(key)
            for field in ("storage_contract", "current_stores", "result", "assertions", "acceptance_scope", "acceptance_when", "acceptance_then"):
                if leaf.get(field) != expected[field]:
                    raise ValueError(f"供应商语义正源存储或失败断言错误: {key} {field}")
            feature = by_entry.get(f"semantic-effect:{key}")
            if feature is None:
                raise ValueError(f"供应商语义叶丢失: {key}")
            for field, value in (("storage_contract", expected["storage_contract"]), ("current_stores", expected["current_stores"]),
                                 ("visible_result", expected["result"]), ("acceptance_assertions", expected["assertions"]),
                                 ("acceptance_scope", expected["acceptance_scope"]), ("acceptance_when", expected["acceptance_when"]),
                                 ("acceptance_then", expected["acceptance_then"])):
                if feature.get(field) != value:
                    raise ValueError(f"供应商逐叶库存存储或失败断言错误: {key} {field}")


def _source_text(path: Path, root: Path, reader: SourceReader | None) -> str:
    relative = str(path.relative_to(root))
    return "\n".join(reader(relative)) if reader else path.read_text(encoding="utf-8")


def _resolve_local_import(source: Path, specifier: str, root: Path) -> Path | None:
    if not specifier.startswith("."):
        return None
    stem = (source.parent / specifier).resolve()
    candidates = [stem, *(Path(str(stem) + ext) for ext in SOURCE_EXTENSIONS)]
    candidates += [stem / ("index" + ext) for ext in SOURCE_EXTENSIONS]
    for candidate in candidates:
        if candidate.is_file() and candidate.suffix in SOURCE_EXTENSIONS and candidate.is_relative_to(root):
            return candidate
    return None


def _component_imports(container: Path, names: set[str], root: Path, reader: SourceReader | None) -> list[Path]:
    code = _source_text(container, root, reader)
    found: list[Path] = []
    for match in re.finditer(r"import\s+(?:\{([^}]+)\}|(\w+))\s+from\s+['\"]([^'\"]+)['\"]", code):
        imported = {part.strip().split(" as ")[-1] for part in (match.group(1) or "").split(",") if part.strip()}
        if match.group(2):
            imported.add(match.group(2))
        if imported & names:
            target = _resolve_local_import(container, match.group(3), root)
            if target:
                found.append(target)
    missing = names - {name for name in names if re.search(rf"\bimport\s+(?:\{{[^}}]*\b{re.escape(name)}\b[^}}]*\}}|{re.escape(name)})\s+from", code)}
    if missing:
        raise ValueError(f"UI 组合根组件无静态导入: {container.relative_to(root)} {sorted(missing)}")
    return found


def ui_consumer_closure(entry_id: str, source_refs: list[str], root: Path, reader: SourceReader | None = None) -> tuple[set[str], list[dict]]:
    """从页面文件追静态模块依赖；未知的动态目标必须显式交人工核查。"""
    if entry_id.startswith("ui:settings:"):
        tab = entry_id.split(":")[-1]
        container = root / "desktop/src/react/settings/SettingsContent.tsx"
        match = re.search(rf"\b{re.escape(tab)}:\s*(\w+)", _source_text(container, root, reader))
        if not match:
            raise ValueError(f"设置页组合根不再挂载: {entry_id}")
        pending = _component_imports(container, {match.group(1)}, root, reader)
        composition_roots = {"desktop/src/react/settings/SettingsContent.tsx", "desktop/src/react/settings/SettingsNav.tsx"}
    elif entry_id.startswith(("ui:page:", "ui:panel:")):
        container = root / "desktop/src/react/components/app/AppPages.tsx"
        identity = entry_id.split(":")[-1]
        if identity == "channels":
            names = {"ChannelMessages", "ChannelMembers", "ChannelInput", "ChannelReadonly", "ChannelAgentActivityPanel", "ChannelAgentSettingsPanel", "ChannelExportPanel", "ChannelHeader"}
        elif identity == "chat":
            names = {"ChatPage", "PreviewPanel", "SideChatPanel"}
        else:
            source_line = int(source_refs[0].rsplit(":", 1)[1])
            line = _source_text(container, root, reader).splitlines()[source_line - 1]
            match = re.search(r"<([A-Z]\w+)", line)
            if not match:
                raise ValueError(f"UI 组合根 JSX 入口变化: {entry_id} {source_line}")
            names = {match.group(1)}
        pending = _component_imports(container, names, root, reader)
        composition_roots = {"desktop/src/react/components/app/AppPages.tsx"}
    else:
        raise ValueError(f"未知 UI 入口类型: {entry_id}")
    if not pending:
        raise ValueError(f"UI 组合根没有可追踪组件: {entry_id}")
    seen: set[Path] = set()
    manual: list[dict] = []
    while pending:
        source = pending.pop().resolve()
        if source in seen:
            continue
        if not source.is_file() or not source.is_relative_to(root):
            raise ValueError(f"UI 消费链源码不可读: {source}")
        seen.add(source)
        code = _source_text(source, root, reader)
        for specifier in IMPORT_RE.findall(code):
            dependency = _resolve_local_import(source, specifier, root)
            if dependency:
                pending.append(dependency)
            elif specifier.startswith(".") and not specifier.endswith((".css", ".scss", ".svg", ".png", ".jpg", ".jpeg", ".webp", ".gif", ".ico", ".woff", ".woff2", ".json")):
                manual.append({"source": str(source.relative_to(root)), "specifier": specifier, "reason": "静态本地依赖无法解析"})
        executable_lines = "\n".join(line for line in code.splitlines() if not line.lstrip().startswith(("//", "*", "/*")))
        if re.search(r"\bimport\s*\(\s*[^'\"\s]", executable_lines):
            manual.append({"source": str(source.relative_to(root)), "reason": "动态 import 目标无法静态枚举"})
        if re.search(r"\brequire\s*\(\s*[^'\"\s]", executable_lines):
            manual.append({"source": str(source.relative_to(root)), "reason": "动态 require 目标无法静态枚举"})
    return {str(path.relative_to(root)) for path in seen} | composition_roots, manual


def ask_user_source_branches(root: Path, reader: SourceReader | None = None,
                             contract_values: list[str] | None = None,
                             include_discovery: bool = False) -> dict:
    """由 TypeScript AST 沿 decision.action 来源求值；未知条件不得自动落入兜底。"""
    source = _source_text(root / "lib/tools/ask-user-tool.ts", root, reader)
    parser = root / "docs/rust-tauri/R00/ask_user_ast_gate.cjs"
    result = subprocess.run(
        ["node", str(parser)], cwd=root,
        input=json.dumps({"source": source, "contractValues": contract_values or []}),
        text=True, capture_output=True, check=False,
    )
    if result.returncode:
        raise ValueError(f"ask_user AST 源码发现失败，需 MANUAL: {result.stderr[-600:]}")
    discovery = json.loads(result.stdout)
    if discovery["manual"]:
        raise ValueError(f"ask_user AST 未知条件 MANUAL，自动通过失败: {discovery['manual'][:4]}")
    return discovery if include_discovery else discovery["branches"]


def validate_ask_user_contract(contract: dict, review: dict, scenario: dict, root: Path, reader: SourceReader | None = None) -> None:
    if contract.get("entry_id") != "tool:ask_user" or contract.get("selector") != "decision?.action":
        raise ValueError("ask_user 人工语义契约入口或选择器错误")
    rows = contract.get("branches", [])
    discovery = ask_user_source_branches(root, reader, [row.get("value") for row in rows], include_discovery=True)
    extracted = discovery["branches"]
    declared = {row.get("value"): row for row in rows}
    if len(declared) != len(rows) or set(declared) != set(extracted):
        raise ValueError(f"ask_user 源码分支与人工语义契约差集: 新增={sorted(set(extracted)-set(declared))}, 失效={sorted(set(declared)-set(extracted))}")
    if contract.get("source_condition_digest") != discovery.get("condition_digest"):
        raise ValueError("ask_user 条件改写尚未进入人工语义契约裁决")
    joined = "；".join(scenario.get("assertions", []))
    for value, row in declared.items():
        if row.get("decision") not in {"SPLIT", "SAME_EFFECT_WITH_VARIANTS", "INTERNAL_STEP", "UNREACHABLE"}:
            raise ValueError(f"ask_user 分支缺行为身份裁决: {value}")
        if row.get("reachability") not in {"REACHABLE", "UNREACHABLE_WITH_PROOF"}:
            raise ValueError(f"ask_user 分支缺可达条件: {value}")
        if not row.get("source_condition") or not row.get("observable_result") or not row.get("scenario_assertion"):
            raise ValueError(f"ask_user 分支缺结果或场景: {value}")
        if row.get("source_result_signature") != extracted[value]:
            raise ValueError(f"ask_user 源码结果与人工语义契约不同: {value} {extracted[value]}")
        if row.get("source_result_digest") != discovery["results"][value].get("digest"):
            raise ValueError(f"ask_user 源码结果依赖与人工语义契约不同: {value}")
        if row["scenario_assertion"] not in joined:
            raise ValueError(f"ask_user 分支没有逐项场景断言: {value}")
        if value != "<default>" and value not in "；".join(review.get("branch_variants", [])):
            raise ValueError(f"ask_user 分支没有进入逐叶审查: {value}")


def validate_ui_source_edges(matrix: dict, feature_by_id: dict[str, dict], root: Path,
                             reader: SourceReader | None = None) -> None:
    """以冻结的源码分流事实和独立调用点核对候选动作；不能靠同步 SHA 消除缺边。"""
    pages = {page["entry_id"]: page for page in matrix["pages"]}
    actions = {action["action_id"]: action for page in matrix["pages"] for action in page["actions"]}
    by_entry = {feature["entry_id"]: feature for feature in feature_by_id.values()}
    helper = _source_text(root / "desktop/src/react/settings/helpers.ts", root, reader)
    if "lingxiFetch(`/api/agents/${agentId}/config`" not in helper or "await refreshSettingsConfigSnapshot()" not in helper:
        raise ValueError("autoSaveConfig PUT 与写后 GET 契约变化，须人工复审")
    per_agent_write = by_entry["semantic-effect:agent.config.update"]
    agent_route_global_write = by_entry["semantic-effect:agent.config.global_update"]
    per_agent_read = by_entry["semantic-effect:agents./agents/.id/config.read"]
    global_write = by_entry["semantic-effect:config.global.update"]
    config_schema = _source_text(root / "shared/config-schema.ts", root, reader)
    declared_global_fields = set(re.findall(r"^\s*['\"]?([\w.]+)['\"]?:\s*\{\s*scope:\s*['\"]global['\"]", config_schema, re.MULTILINE))
    if not declared_global_fields or "capabilities.learn_skills" not in declared_global_fields:
        raise ValueError("CONFIG_SCHEMA 全局字段分流解析变化，须人工复审")
    semantic = json.loads((root / "docs/rust-tauri/R00/R00-T02_SEMANTIC_AUDIT_G3.json").read_text(encoding="utf-8"))
    put_review = next((row for row in semantic["entries"] if row["entry_id"] == "behavior:agents:/agents/:id/config:PUT"), None)
    if put_review is None:
        raise ValueError("agents 配置路由分流审查缺失")
    validate_config_request_cases(put_review.get("request_cases", []), declared_global_fields)
    global_semantic = json.loads((root / "docs/rust-tauri/R00/R00-T02_SEMANTIC_AUDIT_G2.json").read_text(encoding="utf-8"))
    global_review = next((row for row in global_semantic["entries"] if row["entry_id"] == "behavior:config:/config:PUT"), None)
    if global_review is None:
        raise ValueError("全局配置供应商审查缺失")
    validate_provider_leaf_contracts(by_entry, {put_review["entry_id"]: put_review, global_review["entry_id"]: global_review}, root, reader)
    if "preferences.json" not in "；".join(agent_route_global_write.get("current_stores", [])) or "config.yaml" not in "；".join(per_agent_write.get("current_stores", [])):
        raise ValueError("全局与助手配置叶的 current_stores 错挂")
    for leaf, tokens in ((agent_route_global_write, ("纯全局", "跨助手", "mixed patch", "不得宣称原子回滚")),
                         (per_agent_write, ("纯助手", "config.yaml", "mixed patch", "不得宣称原子回滚"))):
        content = leaf.get("visible_result", "") + leaf.get("acceptance_requirement", "")
        if any(token not in content for token in tokens):
            raise ValueError(f"配置逐叶结果或验收断言缺纯/混合边界: {leaf['entry_id']}")
    agent_route = _source_text(root / "server/routes/agents.ts", root, reader)
    scope_source = _source_text(root / "shared/config-scope.ts", root, reader)
    for marker in ("splitByScope(partial)", "engine[setter](value)", "Object.keys(agentPartial).length === 0", "saveConfig(configPath, agentPartial)",
                   "engine.providerRegistry.saveProvider", "clearInlineProviderCredentialFields(block)"):
        if marker not in agent_route:
            raise ValueError(f"助手配置路由持久化分流变化，须人工复审: {marker}")
    for marker in ("delete agent[parts[0]]", "delete agent[parent][child]", "delete agent[parent]"):
        if marker not in scope_source:
            raise ValueError(f"splitByScope 字段移除条件变化，须人工复审: {marker}")
    expected_fields: dict[str, set[str]] = {}
    expected_effects: dict[str, set[str]] = {}
    for action_id, source, marker, paths in SOURCE_AUTOSAVE_CONFIG_REQUESTS:
        source_code = _source_text(root / source, root, reader)
        if marker not in source_code:
            raise ValueError(f"设置控件 autoSaveConfig 源码调用变化，须人工复审: {action_id}")
        if action_id == "ui:settings:skills#07" and not re.search(r"autoSaveConfig\(\s*\{\s*capabilities:\s*\{\s*learn_skills:", source_code):
            raise ValueError("技能能力开关 autoSaveConfig 请求体变化，须人工复审")
        expected_fields.setdefault(action_id, set()).update(paths)
        patch = {}
        for path in paths:
            parts = path.split(".")
            if len(parts) == 1:
                patch[parts[0]] = True
            else:
                patch.setdefault(parts[0], {})[parts[1]] = True
        expected_effects.setdefault(action_id, set()).update(classify_config_patch(patch, declared_global_fields))
    for action_id, fields in expected_fields.items():
        expected = expected_effects[action_id]
        targets = set(actions.get(action_id, {}).get("target_feature_ids", []))
        write_leaves = {per_agent_write["entry_id"]: per_agent_write, agent_route_global_write["entry_id"]: agent_route_global_write}
        actual = {entry for entry, leaf in write_leaves.items() if leaf["feature_id"] in targets}
        if actual != expected or per_agent_read["feature_id"] not in targets or global_write["feature_id"] in targets:
            raise ValueError(f"设置控件 autoSaveConfig 写入 scope 与效果边错挂: {action_id} 预期={sorted(expected)} 实际={sorted(actual)}")
        scenarios = set(actions[action_id].get("target_scenario_ids", []))
        expected_scenarios = {write_leaves[entry]["supplemental_acceptance_id"] for entry in expected}
        actual_scenarios = {leaf["supplemental_acceptance_id"] for leaf in write_leaves.values() if leaf["supplemental_acceptance_id"] in scenarios}
        if actual_scenarios != expected_scenarios or per_agent_read["supplemental_acceptance_id"] not in scenarios:
            raise ValueError(f"设置控件 autoSaveConfig 写入 scope 与逐叶场景错挂: {action_id}")
        scope = actions[action_id].get("storage_scope", {})
        if scope.get("transport") != "PUT /api/agents/:id/config" or scope.get("server_dispatch") != "splitByScope":
            raise ValueError(f"设置控件缺传输入口与持久化范围区分: {action_id}")
        global_fields = set(scope.get("global_preferences_fields", []))
        agent_fields = set(scope.get("agent_config_fields", []))
        if global_fields != fields & declared_global_fields or agent_fields != fields - declared_global_fields:
            raise ValueError(f"设置控件持久化 scope 与请求体/CONFIG_SCHEMA 冲突: {action_id}")
        variants = [row for row in actions[action_id].get("control_variants", []) if row.get("effect_entry_id") in write_leaves]
        if {row.get("effect_entry_id") for row in variants} != expected or len(variants) != len(expected):
            raise ValueError(f"设置控件写入变体与独立请求/效果叶不一致: {action_id}")
        if action_id == "ui:settings:interface#03":
            if not any("editor" in row.get("visible_or_automatic_action", "") and row.get("effect_entry_id") == agent_route_global_write["entry_id"] for row in variants) or not any("chat" in row.get("visible_or_automatic_action", "") and row.get("effect_entry_id") == per_agent_write["entry_id"] for row in variants):
                raise ValueError("interface#03 排版 editor 与聊天 chat 两个独立请求未分别归属")
            request_variants = scope.get("request_variants", [])
            expected_requests = {
                ("editor", agent_route_global_write["entry_id"], "desktop/src/react/settings/tabs/InterfaceTab.tsx:233"),
                ("chat", per_agent_write["entry_id"], "desktop/src/react/settings/tabs/InterfaceTab.tsx:253"),
            }
            actual_requests = {(row.get("patch_field"), row.get("effect_entry_id"), row.get("source_ref")) for row in request_variants}
            if actual_requests != expected_requests or len(request_variants) != 2 or any(row.get("same_request") is not False for row in request_variants):
                raise ValueError("interface#03 两个控件的独立请求体/存储归属不完整")
    for action_id, effect_entry, source, marker in SOURCE_REQUIRED_UI_EDGES:
        if marker not in _source_text(root / source, root, reader):
            raise ValueError(f"UI 源码分流条件或效果调用已变化，须人工复审: {action_id} {source} {marker}")
        target = by_entry.get(effect_entry)
        if not target or action_id not in actions or target["feature_id"] not in actions[action_id].get("target_feature_ids", []):
            raise ValueError(f"UI 源码已存在的效果边在逐动作候选缺席: {action_id} → {effect_entry}")
        if target["supplemental_acceptance_id"] not in actions[action_id].get("target_scenario_ids", []):
            raise ValueError(f"UI 源码效果边没有逐叶场景: {action_id} → {effect_entry}")
    for action_id, effect_entry in SOURCE_FORBIDDEN_UI_EDGES:
        target = by_entry.get(effect_entry)
        if target and target["feature_id"] in actions.get(action_id, {}).get("target_feature_ids", []):
            raise ValueError(f"UI 控件错挂未由现役源码调用的效果边: {action_id} → {effect_entry}")

    # 自动化卡片同一开关有两种 HTTP 效果及一条本地拒绝路径。
    card = re.sub(r"\s+", "", _source_text(root / "desktop/src/react/components/automation/AutomationCard.tsx", root, reader))
    panel = re.sub(r"\s+", "", _source_text(root / "desktop/src/react/components/AutomationPanel.tsx", root, reader))
    server = re.sub(r"\s+", "", _source_text(root / "server/routes/desk.ts", root, reader))
    required_card = (
        "consttoggleEnabled=async()=>{if(job.enabled){onToggleEnabled(job.id);return;}",
        "if(isAgentSession&&!prompt.trim()){addToast(t('automation.promptRequired'),'error');return;}",
        "awaitonUpdate(job.id,{...updateFields(),enabled:true});",
        "constsave=async()=>{constfields=updateFields();if(Object.keys(fields).length)awaitonUpdate(job.id,fields);}",
        "if(label!==jobTitle(job))fields.label=label;",
        "if(scheduleDirty){",
        "if(prompt!==(job.prompt||''))fields.prompt=prompt;",
        "if(model!==modelSelectValue(job.model))fields.model=modelValueFromSelect(model);",
        "voidtoggleEnabled();",
    )
    required_panel = (
        "body:JSON.stringify({action:'toggle',id:jobId})",
        "body:JSON.stringify({action:'update',id:jobId,...fields})",
        "onToggleEnabled={toggleJob}", "onUpdate={updateJob}",
    )
    required_server = (
        'case"toggle":{', "job=store.toggleJob(params.id);",
        'case"update":{', "job=store.updateJob(id,fields);",
    )
    for source, content, markers in (("AutomationCard.tsx", card, required_card),
                                     ("AutomationPanel.tsx", panel, required_panel),
                                     ("server/routes/desk.ts", server, required_server)):
        for marker in markers:
            if marker not in content:
                raise ValueError(f"UI 自动化源码状态分流/草稿合并变化，须重新裁决: {source} {marker}")
    enable = by_entry["semantic-effect:cron.update"]
    disable = by_entry["semantic-effect:cron.toggle"]
    switch = actions.get("ui:panel:automation#03")
    save = actions.get("ui:panel:automation#05")
    if not switch or set(switch.get("target_feature_ids", [])) != {enable["feature_id"], disable["feature_id"]}:
        raise ValueError("UI 自动化 #03 启用 update 与停用 toggle 的效果边不完整或错挂")
    if {row.get("target_feature_id") for row in switch.get("control_variants", [])} != {enable["feature_id"], disable["feature_id"]}:
        raise ValueError("UI 自动化 #03 两条状态变体缺失")
    expected_branches = {
        ("job.enabled === false && isAgentSession && !prompt.trim()", "LOCAL_BLOCK"),
        ("job.enabled === false && prompt eligible", enable["entry_id"]),
        ("job.enabled === true", disable["entry_id"]),
    }
    if {(row.get("condition"), row.get("effect")) for row in switch.get("state_effect_branches", [])} != expected_branches:
        raise ValueError("UI 自动化 #03 状态条件→本地拒绝/update/toggle 与现役源码不符")
    if not save or set(save.get("target_feature_ids", [])) != {enable["feature_id"]} or "有变更才发送" not in save.get("success", ""):
        raise ValueError("UI 自动化 #05 独立保存的有变更才 update 语义失效")
    # #03 的无草稿启用和合并草稿是本页断言，不能由 #05 代验。
    result = "；".join(switch.get(key, "") for key in ("success", "empty_or_uninitialized", "failure_or_forbidden"))
    if not all(token in result for token in ("enabled:true", "无变化", "未保存草稿", "action:update", "action:toggle")):
        raise ValueError("UI 自动化 #03 本页场景缺启用/停用、无草稿及草稿合并结果")

    contract_path = root / "docs/rust-tauri/R00/R00-T02_UI_SOURCE_BRANCH_CONTRACT.json"
    contract = json.loads(contract_path.read_text(encoding="utf-8"))
    contract_pages = {row.get("entry_id"): row for row in contract.get("pages", [])}
    if len(contract_pages) != 26 or set(contract_pages) != set(pages):
        raise ValueError("UI 源码→候选人工契约没有覆盖全部 26 页")
    for entry_id, page in pages.items():
        facts = contract_pages[entry_id].get("facts", [])
        declared = {(fact.get("action_id"), fact.get("target_feature_id"), fact.get("branch")) for fact in facts}
        if len(declared) != len(facts) or not facts:
            raise ValueError(f"UI 源码事实重复或缺失: {entry_id}")
        expected = set()
        for action in page["actions"]:
            variants = action.get("control_variants") or [
                {"target_feature_id": fid, "visible_or_automatic_action": action["trigger"]}
                for fid in action.get("target_feature_ids", [])
            ]
            for variant in variants:
                expected.add((action["action_id"], variant["target_feature_id"], variant["visible_or_automatic_action"]))
        if declared != expected:
            raise ValueError(f"UI 现役源码事实与候选控件状态→目标叶双向差集: {entry_id} 源码独有={sorted(declared-expected)[:3]} 候选独有={sorted(expected-declared)[:3]}")
        for fact in facts:
            action = actions[fact["action_id"]]
            target = feature_by_id.get(fact["target_feature_id"])
            if not target or fact.get("target_scenario_id") != target["supplemental_acceptance_id"]:
                raise ValueError(f"UI 源码事实没有对应逐叶场景: {entry_id} {fact['action_id']}")
            if fact.get("resolution") not in {"SOURCE_CITED", "MANUAL"} or not fact.get("source_refs"):
                raise ValueError(f"UI 源码事实缺可核调用点或 MANUAL 裁决: {entry_id} {fact['action_id']}")
            for locator in fact["source_refs"]:
                match = re.fullmatch(r"(.+):(\d+)", locator)
                if not match or not (root / match.group(1)).is_file():
                    raise ValueError(f"UI 源码事实定位无效: {entry_id} {locator}")
            if fact["resolution"] == "MANUAL" and not fact.get("manual_reason"):
                raise ValueError(f"UI 未知动态边界没有人工说明: {entry_id} {fact['action_id']}")

    # 旧独立审查的整文件 SHA 属于 R8 冻结版本。R9 只复用其中完全相同的
    # Agent/Providers 页面子树和逐变体事实；其余页面仍待新审查者裁决。
    audit_dir = root / "docs/rust-tauri/R00"
    agent_provider_audit = json.loads((audit_dir / "R00-T02_UI_AGENT_PROVIDERS_AUDIT.json").read_text(encoding="utf-8"))
    frozen_pages = agent_provider_audit["final_same_version_snapshot"]["page_subtree_sha256"]
    for entry_id in ("ui:settings:agent", "ui:settings:providers"):
        subtree = json.dumps(pages[entry_id], ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")
        if hashlib.sha256(subtree).hexdigest() != frozen_pages[entry_id]:
            raise ValueError(f"Agent/Providers 独立复核页面子树已漂移，须重新审查: {entry_id}")
    if agent_provider_audit.get("decision_counts") != {"SOURCE_CITED_CORRECT": 243} or agent_provider_audit.get("action_decision_counts") != {"SOURCE_CITED_CORRECT": 33}:
        raise ValueError("Agent/Providers 独立逐变体复核仍有 WRONG/MANUAL")
    reviewed = agent_provider_audit["variants"]
    if len(reviewed) != 243 or any(row.get("decision") != "SOURCE_CITED_CORRECT" for row in reviewed):
        raise ValueError("Agent/Providers 独立逐变体复核缺项")
    for entry_id in ("ui:settings:agent", "ui:settings:providers"):
        if any(fact["resolution"] != "SOURCE_CITED" for fact in contract_pages[entry_id]["facts"]):
            raise ValueError(f"Agent/Providers 已复核页仍留 MANUAL: {entry_id}")
    reviewed_keys = {(row["action_id"], row["target_feature_id"], row["target_scenario_id"], None if row["synthetic_action_leaf"] else row["visible_or_automatic_action"]) for row in reviewed}
    contract_keys = {(fact["action_id"], fact["target_feature_id"], fact["target_scenario_id"], fact["branch"] if actions[fact["action_id"]].get("control_variants") else None) for entry_id in ("ui:settings:agent", "ui:settings:providers") for fact in contract_pages[entry_id]["facts"]}
    if reviewed_keys != contract_keys or len(reviewed_keys) != 243:
        raise ValueError("Agent/Providers 独立逐叶审查与源码契约差集")

    # 两份独立只读审查和 67 行结果复核逐项消差，OPEN 不能作为新候选交付。
    first = json.loads((audit_dir / "R00-T02_UI_BRANCH_REVIEW.json").read_text(encoding="utf-8"))
    second = json.loads((audit_dir / "R00-T02_UI_BRANCH_REVIEW_B.json").read_text(encoding="utf-8"))
    drift = json.loads((audit_dir / "R00-T02_RESULT_DRIFT_REVIEW.json").read_text(encoding="utf-8"))
    ledger = json.loads((audit_dir / "R00-T02_R8_CLOSURE_LEDGER.json").read_text(encoding="utf-8"))
    expected_ids = {f"A{i:02d}" for i in range(1, len(first["confirmed_baseline_missing_edges"]) + 1)}
    expected_ids |= {f"A-IPC{i:02d}" for i in range(1, len(first["settings_changed_ipc_missing"]["action_source_refs"]) + 1)}
    expected_ids |= {row["id"] for row in second["confirmed_missing_edges"]}
    findings = {row["id"]: row for row in ledger.get("ui_findings", [])}
    if set(findings) != expected_ids or len(findings) != len(ledger.get("ui_findings", [])):
        raise ValueError("R8 独立 UI 缺边逐项账表与 A/B 审查差集")
    for finding in findings.values():
        if finding.get("decision") not in {"CLOSED", "MANUAL_WITH_REASON"} or not finding.get("candidate_checks"):
            raise ValueError(f"R8 已确证缺边仍 OPEN 或无具体检查: {finding['id']}")
        for check in finding["candidate_checks"]:
            action = actions.get(check["action_id"])
            target_id = check["feature_id"]
            if not action or (target_id in action.get("target_feature_ids", [])) != check.get("target_present", not check.get("target_absent", False)):
                raise ValueError(f"R8 独立审查效果边回归: {finding['id']} {check['action_id']}")
            if check.get("target_present"):
                target = feature_by_id.get(target_id)
                if not target or target["supplemental_acceptance_id"] not in action.get("target_scenario_ids", []):
                    raise ValueError(f"R8 独立审查逐叶场景回归: {finding['id']} {check['action_id']}")
            elif not check.get("target_absent"):
                raise ValueError(f"R8 独立审查多挂错误效果边: {finding['id']} {check['action_id']}")
    result_rows = {row["action_id"]: row for row in ledger.get("result_drift", [])}
    expected_result_ids = {row["action_id"] for row in drift["decisions"]}
    if len(result_rows) != len(drift["decisions"]) or set(result_rows) != expected_result_ids:
        raise ValueError("R8 旧结果 67 行逐项账表缺失或重复")
    for action_id, row in result_rows.items():
        action = actions[action_id]
        if row.get("decision") != "CLOSED" or any(row.get(f"current_{key}") != action.get(key)
                                                   for key in ("empty_or_uninitialized", "failure_or_forbidden")):
            raise ValueError(f"R8 旧结果文字未逐字段裁决: {action_id}")
    # 后续独立复核新增的错挂、漏边和结果问题也进入同一关闭账表。
    followup = ledger.get("followup_findings", [])
    followup_ids = {row.get("id") for row in followup}
    skills_usage = json.loads((audit_dir / "R00-T02_UI_SKILLS_USAGE_AUDIT.json").read_text(encoding="utf-8"))
    expected_followup = {f"BM{i:02d}" for i in range(1, 4)}
    expected_followup |= {f"SU:{item['id']}" for item in skills_usage["findings"]}
    expected_followup |= {f"SH{i:02d}" for i in range(1, 6)}
    expected_followup |= {f"AP-R{i:02d}" for i in range(1, 6)}
    expected_followup |= {f"AP-C{i:02d}" for i in range(1, 8)}
    expected_followup |= {f"AP-FIXED-{i:02d}" for i in range(1, 6)}
    if followup_ids != expected_followup or len(followup_ids) != len(followup):
        raise ValueError("R8 后续独立审查关闭账表缺项或重复")
    feature_tail = {fid[-6:]: fid for fid in feature_by_id}
    for item in followup:
        if item.get("decision") != "CLOSED" or not item.get("candidate_checks"):
            raise ValueError(f"R8 后续独立审查未闭合: {item.get('id')}")
        for check in item["candidate_checks"]:
            action = actions.get(check.get("action_id"))
            if not action:
                raise ValueError(f"R8 后续独立审查动作缺席: {item['id']}")
            kind, value = check.get("kind"), check.get("value")
            if kind in {"present", "absent"}:
                target = feature_tail.get(value)
                actual = bool(target and target in action.get("target_feature_ids", []))
                actual = actual if kind == "present" else not actual
            elif kind in {"success", "failure", "empty", "trigger"}:
                field = {"success": "success", "failure": "failure_or_forbidden",
                         "empty": "empty_or_uninitialized", "trigger": "trigger"}[kind]
                actual = isinstance(value, str) and value in action.get(field, "")
            elif kind == "variant":
                actual = any(value in row.get("visible_or_automatic_action", "")
                             for row in action.get("control_variants", []))
            elif kind == "scope_global":
                actual = value in action.get("storage_scope", {}).get("global_preferences_fields", [])
            else:
                raise ValueError(f"R8 后续独立审查检查类型未知: {item['id']} {kind}")
            if not actual or check.get("passed") is not True:
                raise ValueError(f"R8 后续独立审查回归: {item['id']} {check.get('action_id')} {value}")


def validate_ui_review_fields(matrix: dict, reviews: dict[str, dict]) -> None:
    """同一当前结果只能有一份真相；页面汇总和逐叶审查必须随动作同步。"""
    for page in matrix["pages"]:
        entry_id = page["entry_id"]
        actions = page["actions"]
        current_evidence = "；".join(page.get("consumer_closure", {}).get("evidence", []))
        if (entry_id == "ui:panel:activity" and "活动升级为自动化" in current_evidence
                or entry_id == "ui:settings:usage" and "八卡" in current_evidence):
            raise ValueError(f"UI 当前消费链仍保留已否定的旧结果文字: {entry_id}")
        summary = page.get("error_empty_paths", [])
        if len(summary) != len(actions) or [row.get("action_id") for row in summary] != [row["action_id"] for row in actions]:
            raise ValueError(f"UI 页面空态/错误态逐动作行数或 ID 漂移: {entry_id}")
        for action, row in zip(actions, summary):
            for key in ("empty_or_uninitialized", "failure_or_forbidden"):
                if row.get(key) != action.get(key):
                    raise ValueError(f"UI 页面旧文字与现行动作字段冲突: {action['action_id']} {key}")
        review = reviews.get(page["original_feature_id"])
        if review is None:
            raise ValueError(f"UI 页面没有非 HTTP 逐叶审查: {entry_id}")
        expected = {
            "user_action": "；".join(action["trigger"] for action in actions),
            "success_result": "；".join(f"{action['trigger']}：{action['success']}" for action in actions),
            "refusal_or_boundary": "；".join(
                f"{action['trigger']}：空态/未初始化 {action['empty_or_uninitialized']}；失败/无权限 {action['failure_or_forbidden']}"
                for action in actions),
        }
        for key, value in expected.items():
            if review.get(key) != value:
                raise ValueError(f"UI 非 HTTP 当前结果字段与逐动作候选冲突: {entry_id} {key}")
        assertions = [f"{action['trigger']}：成功 {action['success']}；空态/未初始化 {action['empty_or_uninitialized']}；失败/无权限 {action['failure_or_forbidden']}" for action in actions]
        if review.get("scenario", {}).get("assertions") != assertions:
            raise ValueError(f"UI 逐叶本页场景与逐动作当前结果冲突: {entry_id}")


def validate_ui_matrix(matrix: dict, ui_features: list[dict], feature_by_id: dict[str, dict], root: Path, reader: SourceReader | None = None) -> dict[str, set[str]]:
    pages = matrix.get("pages", [])
    expected = {feature["entry_id"]: feature for feature in ui_features}
    actual = {page.get("entry_id"): page for page in pages}
    if len(actual) != len(pages) or set(actual) != set(expected) or matrix.get("page_count") != len(expected):
        raise ValueError(f"26 个 UI 页面动作矩阵差集: 缺少={sorted(set(expected)-set(actual))}, 多出={sorted(set(actual)-set(expected))}")
    required_sources: dict[str, set[str]] = {}
    for entry_id, page in actual.items():
        feature = expected[entry_id]
        if page.get("original_feature_id") != feature["feature_id"]:
            raise ValueError(f"UI 页面身份错误: {entry_id}")
        closure, manual = ui_consumer_closure(entry_id, feature["source_ref"], root, reader)
        declared_closure = set(page.get("consumer_closure", {}).get("files", []))
        if not declared_closure or any(not (root / source).is_file() for source in declared_closure):
            raise ValueError(f"UI 人工消费链缺失或文件不存在: {entry_id}")
        declared_sha = page["consumer_closure"].get("sha256", {})
        if set(declared_sha) != declared_closure or any(hashlib.sha256((root / source).read_bytes()).hexdigest() != digest
                                                       for source, digest in declared_sha.items()):
            raise ValueError(f"UI 人工消费链源码版本与报告不符: {entry_id}")
        if manual and not page.get("unresolved_manual"):
            raise ValueError(f"UI 动态边界未标 MANUAL: {entry_id} {manual[:3]}")
        for boundary in page.get("unresolved_manual", []):
            if boundary.get("resolution") != "MANUAL" or not boundary.get("reason") or not boundary.get("suggested_task_id"):
                raise ValueError(f"UI 动态边界缺人工复核归属: {entry_id}")
            locator = boundary.get("location", "")
            match = re.fullmatch(r"(.+):(\d+)", locator)
            if not match or not (root / match.group(1)).is_file():
                raise ValueError(f"UI 动态边界调用点无效: {entry_id} {locator}")
        actions = page.get("actions", [])
        if not actions or len({action.get("action_id") for action in actions}) != len(actions):
            raise ValueError(f"UI 页面缺动作或动作 ID 重复: {entry_id}")
        targets: set[str] = set()
        for action in actions:
            status = action.get("resolution")
            if status not in {"MAPPED", "GAP", "MANUAL"}:
                raise ValueError(f"UI 动作未裁决: {entry_id} {action.get('action_id')}")
            if status == "GAP":
                raise ValueError(f"UI 可见动作没有实际目标叶/场景: {entry_id} {action.get('action_id')} {action.get('gap_reason')}")
            if not action.get("trigger") or not action.get("call_chain"):
                raise ValueError(f"UI 动作缺实际触发/调用链: {entry_id} {action.get('action_id')}")
            if action.get("ui_result_scenario_id") != feature["supplemental_acceptance_id"]:
                raise ValueError(f"UI 动作自身四态结果未挂本页验收场景: {entry_id} {action.get('action_id')}")
            for source_ref in action["call_chain"]:
                match = re.fullmatch(r"(.+):(\d+)", source_ref)
                source = match.group(1) if match else source_ref
                if not match or not (root / source).is_file() or int(match.group(2)) > len(_source_text(root / source, root, reader).splitlines()):
                    raise ValueError(f"UI 动作调用链源码不存在: {entry_id} {source_ref}")
            if status == "MAPPED":
                fids = set(action.get("target_feature_ids", []))
                sids = set(action.get("target_scenario_ids", []))
                if not fids or not fids <= feature_by_id.keys() or sids != {feature_by_id[fid]["supplemental_acceptance_id"] for fid in fids}:
                    raise ValueError(f"UI 动作真实目标叶/场景不匹配: {entry_id} {action.get('action_id')}")
                chain_sources = {ref.rsplit(":", 1)[0] for ref in action["call_chain"]}
                for fid in fids - {feature["feature_id"]}:
                    target_sources = {ref.rsplit(":", 1)[0] for ref in feature_by_id[fid]["source_ref"]}
                    if not chain_sources & target_sources:
                        raise ValueError(f"UI 动作调用链与所挂目标叶无源码交点: {entry_id} {action.get('action_id')} {fid}")
                variants = action.get("control_variants", [])
                if variants and {variant.get("target_feature_id") for variant in variants} != fids:
                    raise ValueError(f"UI 同动作控件变体与目标叶不一致: {entry_id} {action.get('action_id')}")
                for variant in variants:
                    target = feature_by_id[variant["target_feature_id"]]
                    if (variant.get("effect_entry_id") != target["entry_id"]
                            or variant.get("target_scenario_id") != target["supplemental_acceptance_id"]
                            or not variant.get("visible_or_automatic_action")
                            or not variant.get("effect_source_refs")):
                        raise ValueError(f"UI 控件变体缺真实效果/场景/源码: {entry_id} {action.get('action_id')}")
                targets.update(fids - {feature["feature_id"]})
            elif not action.get("gap_reason"):
                raise ValueError(f"UI 动作缺口/人工边界没有说明: {entry_id} {action.get('action_id')}")
            for key in ("success", "empty_or_uninitialized", "failure_or_forbidden"):
                if not action.get(key):
                    raise ValueError(f"UI 动作缺正反可见结果: {entry_id} {action.get('action_id')} {key}")
        if targets != set(feature.get("supported_feature_ids", [])):
            raise ValueError(f"UI 支持目标与逐动作调用链不一致: {entry_id} 缺少={sorted(targets-set(feature.get('supported_feature_ids', [])))}, 多出={sorted(set(feature.get('supported_feature_ids', []))-targets)}")
        required_sources[entry_id] = closure | declared_closure | {
            ref.rsplit(":", 1)[0] for action in actions for ref in action["call_chain"]
        }
    validate_ui_source_edges(matrix, feature_by_id, root, reader)
    return required_sources
