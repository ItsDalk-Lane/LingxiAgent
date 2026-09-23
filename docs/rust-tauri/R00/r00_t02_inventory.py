#!/usr/bin/env python3
"""从当前生产注册点重建 R00-T02 功能清单；默认只校验，--write 重生交付物。"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
from functools import lru_cache
from pathlib import Path

from r00_t02_source_gates import ui_consumer_closure, validate_ask_user_contract, validate_ui_matrix, validate_ui_review_fields, validate_ui_source_edges, validate_config_request_cases, validate_provider_leaf_contracts

ROOT = Path(__file__).resolve().parents[3]
OUT = Path(__file__).resolve().parent
TASKBOOK = ROOT / "Lingxi_Rust_Tauri_Taskbooks_2026-09-23"

DOMAINS = {
    "D01": ("聊天与输入", "Rust Run/Session＋React", "session JSONL；input-drafts", ["R03-T01", "R06-T04", "R08-T07"]),
    "D02": ("会话管理", "Rust Session/Storage", "session-manifest.db；session JSONL", ["R06-T03", "R06-T04", "R08-T07"]),
    "D03": ("工具目录", "Rust Registry/Gateway", "工具目录配置；会话工具快照", ["R04-T01", "R04-T02"]),
    "D04": ("权限沙盒", "Rust Policy＋OS helper", "权限偏好；批准记录；授权文件夹", ["R04-T03", "R04-T06"]),
    "D05": ("子代理", "Rust Supervisor", "subagent-runs.json；subagent-threads.json", ["R03-T03", "R03-T06", "R06-T03"]),
    "D06": ("人格/MOOD", "Rust Context＋规范化投影", "agent identity.md/AGENTS.md/config；session JSONL", ["R06-T01", "R06-T06"]),
    "D07": ("记忆", "Rust Memory", "agents/{id}/memory/facts.db；memory 文件", ["R06-T06"]),
    "D08": ("知识库", "Rust Knowledge＋受控解析器", "knowledge/knowledge.db；原始来源与索引", ["R06-T07"]),
    "D09": ("文件资源", "Rust Resource", "SessionFile sidecar；资源库；工作区文件", ["R04-T04", "R06-T05"]),
    "D10": ("模型配置", "Rust Model/Config", "provider-catalog.json；agent config", ["R05-T01", "R07-T08"]),
    "D11": ("凭证登录", "Rust Credential", "auth.json；受控凭证存储", ["R05-T02"]),
    "D12": ("模型与媒体", "Rust ModelGateway＋编码worker", "模型配置；媒体任务及生成文件", ["R05-T03", "R05-T06", "R07-T06"]),
    "D13": ("终端", "Rust Process/Tool", "会话命令记录；PTY 进程", ["R04-T05"]),
    "D14": ("浏览器", "Rust BrowserPort＋独立宿主", "浏览器会话；cookie/profile；下载文件", ["R01-T04", "R07-T04", "R09-T05"]),
    "D15": ("系统操作", "Tauri/OS helper＋Rust授权", "系统权限；截图/录音资源", ["R09-T04"]),
    "D16": ("内置插件", "Rust登记/网关＋受控worker", "内置插件 manifest；plugin-data；产物", ["R04-T07", "R07-T05"]),
    "D17": ("Bridge", "Rust入口/运行＋SDK worker", "bridge 配置；bridge 会话", ["R07-T02"]),
    "D18": ("后台自动化", "Rust Scheduler/Supervisor", "cron 任务；heartbeat registry；任务收据", ["R03-T06", "R07-T01"]),
    "D19": ("协作与工作台", "Rust各服务复用共同运行边界", "频道/DM；工作台；workflow/goal 状态", ["R07-T03", "R07-T07"]),
    "D20": ("非桌面入口", "Rust service＋客户端", "设备凭证；服务身份；Web/CLI 会话", ["R02-T03", "R07-T09"]),
    "D21": ("Skills/角色卡", "Rust Skill/Config＋解析worker", "skills；bundles；cards；agent 配置", ["R06-T06", "R07-T08"]),
    "D22": ("用量观测", "Rust Trace/Usage", "model-observability/observability.sqlite", ["R05-T07", "R07-T10"]),
    "D23": ("桌面体验", "Tauri DesktopHost＋原React", "桌面偏好；窗口状态；五语言资源", ["R09-T03", "R09-T07"]),
    "D24": ("构建发布", "构建/更新＋Rust启动门禁", "release-digest；签名/更新状态；数据 epoch", ["R10-T07", "R11-T04"]),
}
LEGACY_MATRIX_DOMAINS = {
    "F01": ["D01", "D02"], "F02": ["D02"], "F03": ["D03"], "F04": ["D04"],
    "F05": ["D05"], "F06": ["D06"], "F07": ["D07"], "F08": ["D08"],
    "F09": ["D09"], "F10": ["D10", "D11", "D12", "D16"],
    "F11": ["D13", "D14", "D15", "D23"], "F12": ["D17"],
    "F13": ["D19", "D20"], "F14": ["D20"], "F15": ["D18", "D19"],
    "F16": ["D22"], "F17": ["D21", "D22"], "F18": ["D24"],
    "F19": ["D16", "D21"], "F20": ["D23"],
}

# 生产 route factory 到功能域的归属。新增已挂载 route 没有归属会直接失败。
ROUTE_DOMAINS = {
    "chat": "D01", "input-drafts": "D01", "sessions": "D02", "conversation-map": "D02", "session-projects": "D02", "checkpoints": "D02", "session-collab": "D19",
    "models": "D10", "config": "D07", "providers": "D10", "auth": "D11", "media": "D12", "speech-recognition": "D12",
    "memory-dream": "D07", "knowledge": "D08", "upload": "D09", "fs": "D09", "resources": "D09", "resource-io": "D09", "file-history": "D09",
    "agents": "D06", "skills": "D21", "character-cards": "D21", "cards": "D21", "avatar": "D06", "experiments": "D05", "bridge": "D17",
    "devices": "D20", "web-auth": "D20", "ws-auth": "D20", "server-identity": "D20", "mobile-static": "D20", "mobile-workbench": "D20", "access": "D20",
    "channels": "D19", "dm": "D19", "desk": "D19", "studio-workspaces": "D19", "diary": "D19", "git-environment": "D19",
    "preferences": "D23", "settings-snapshot": "D23", "commands": "D23", "env-deps": "D23", "html-preview": "D23",
    "confirm": "D04", "mcp": "D03", "usage": "D22", "model-observability": "D22",
}

ROUTE_STORES = {
    "chat": "session JSONL；运行状态/事件流", "input-drafts": "input-drafts 持久化", "sessions": "session-manifest.db；session JSONL；会话元数据", "conversation-map": "会话地图布局持久化；session JSONL", "session-projects": "session-projects 持久化", "checkpoints": "会话 checkpoint 快照", "session-collab": "会话协作状态",
    "models": "模型配置/选择状态", "config": "agent config；用户偏好；memory 文件", "providers": "provider-catalog.json", "auth": "auth.json；OAuth 会话状态", "media": "媒体任务记录；generated 文件", "speech-recognition": "语音模型配置",
    "memory-dream": "agent dream 记录与修订", "knowledge": "knowledge/knowledge.db；原始来源文件", "upload": "会话上传文件/SessionFile", "fs": "经授权的实际文件", "resources": "资源登记与实际文件", "resource-io": "经授权的实际文件；监听事件", "file-history": "工作区文件版本/快照",
    "agents": "agents/{id}/config 与人格/记忆文件", "skills": "skills；bundles；agent 技能配置", "character-cards": "角色卡包/agent 配置", "cards": "角色卡文件", "avatar": "agent/user 头像文件", "experiments": "实验开关配置", "bridge": "bridge 配置/会话/媒体", "devices": "devices.json；device-credentials.json",
    "web-auth": "Web 登录会话/凭证", "ws-auth": "短期 WS ticket", "server-identity": "服务实例身份", "mobile-static": "renderer 静态产物或指引页", "mobile-workbench": "工作区文件；设备会话", "access": "LAN/设备/账号凭证配置",
    "channels": "频道/消息/成员记录", "dm": "DM 会话记录", "desk": "desk 文件树/活动/cron 记录", "studio-workspaces": "Studio workspace mount 配置", "diary": "日记文件", "git-environment": "用户工作树和 Git 仓库",
    "preferences": "全局用户偏好", "settings-snapshot": "配置快照（读取多个设置源）", "commands": "命令登记；无直接持久化", "env-deps": "系统依赖探测缓存", "html-preview": "临时 HTML 预览与资源",
    "confirm": "短期批准记录", "mcp": "plugin-data/mcp 连接器配置及运行状态", "usage": "model-observability/observability.sqlite", "model-observability": "model-observability/observability.sqlite",
}

UI_DOMAINS = {
    "chat": "D01", "map": "D02", "channels": "D19", "agent": "D06", "me": "D23", "interface": "D23", "keybindings": "D23", "general": "D23", "browser": "D14", "work": "D19", "skills": "D21", "mcp": "D03", "bridge": "D17", "providers": "D10", "models": "D10", "usage": "D22", "sharing": "D20", "access": "D20", "experiments": "D05", "security": "D04", "envdeps": "D23", "about": "D24",
}

TOOL_DOMAINS = {
    "read": "D09", "write": "D09", "edit": "D09", "grep": "D09", "find": "D09", "ls": "D09", "file": "D09", "materialize": "D09", "stage_files": "D09",
    "exec_command": "D13", "write_stdin": "D13", "terminal": "D13", "run_code": "D13", "ast_edit": "D13", "ast_grep": "D13", "lsp": "D13",
    "search_memory": "D07", "pin_memory": "D07", "unpin_memory": "D07", "record_experience": "D07", "recall_experience": "D07", "tenet_propose": "D07", "context_notes": "D07",
    "knowledge_search": "D08", "knowledge_read": "D08", "knowledge_outline": "D08", "knowledge_grep": "D08", "knowledge_manage": "D08",
    "web_search": "D14", "web_fetch": "D14", "browser": "D14", "computer": "D15",
    "subagent": "D05", "subagent_reply": "D05", "subagent_close": "D05", "check_pending_tasks": "D05", "stop_task": "D05",
    "automation": "D18", "notify": "D18", "todo_write": "D01", "ask_user": "D01", "current_status": "D01", "session": "D02", "checkpoint": "D02", "rewind": "D02", "session_folders": "D02",
    "channel": "D19", "dm": "D19", "workflow": "D19", "goal": "D19", "loop_control": "D19", "security_scan": "D04",
    "install_skill": "D21", "learn_lesson": "D21", "hana_card_guide": "D21", "show_card": "D21",
    "update_settings": "D10", "run_tools": "D03", "beautify": "D16", "office": "D16",
}

WS_DOMAINS = {"terminal_snapshot_request": "D13", "terminal_tail_request": "D13", "terminal_close_request": "D13", "subagent_stop_request": "D05", "abort": "D01", "steer": "D01", "resume_stream": "D01", "context_usage": "D22", "slash": "D01", "compact": "D01", "prompt": "D01", "interject": "D01"}

# 这两个闭集是对生产注册的分类，不是抽取名单。新增注册必须显式归属。
IPC_DOMAINS = {
    "ack-announcement": "D24", "app-ready": "D23", "app:restart": "D23", "auto-update-check": "D24", "auto-update-install": "D24", "auto-update-set-channel": "D24", "auto-update-state": "D24",
    "browser-close-tab": "D14", "browser-emergency-stop": "D14", "browser-go-back": "D14", "browser-go-forward": "D14", "browser-new-tab": "D14", "browser-reload": "D14", "browser-switch-tab": "D14", "close-browser-viewer": "D14", "open-browser-viewer": "D14",
    "copy-file": "D09", "debug-open-onboarding": "D23", "debug-open-onboarding-preview": "D23", "get-app-version": "D24", "get-auto-launch-status": "D23", "get-avatar-path": "D06", "get-keep-awake-status": "D23", "get-pending-announcement": "D24", "get-platform": "D23", "get-server-port": "D20", "get-server-token": "D20", "get-splash-info": "D23", "get-update-digest-history": "D24", "keybindings:reload-global": "D23", "keybindings:test-register": "D23",
    "observability-export:abort": "D22", "observability-export:begin": "D22", "observability-export:end": "D22", "observability-export:write": "D22", "onboarding-complete": "D23", "open-external": "D23", "open-file": "D09", "open-folder": "D09", "open-settings": "D23", "open-skill-viewer": "D21", "quick-chat-hide": "D01", "quick-chat-open-session": "D01", "quick-chat-reload-shortcut": "D23", "quick-chat-resize": "D01", "quick-chat-shortcut-status": "D23", "quick-chat-show": "D01",
    "read-docx-html": "D23", "read-file": "D09", "read-file-base64": "D09", "read-file-snapshot": "D09", "read-xlsx-html": "D23", "release-check-latest": "D24", "reload-main-window": "D23", "run-edit-command": "D09", "screenshot-render": "D15", "select-files": "D09", "select-folder": "D09", "select-skill": "D21", "set-auto-launch-enabled": "D23", "set-keep-awake-enabled": "D23", "show-in-finder": "D09", "show-notification": "D23", "skill-viewer-list-files": "D21", "skill-viewer-read-file": "D21", "spawn-viewer": "D23", "speech-permission-request": "D15", "speech-permission-status": "D15",
    "train-fallback-notice-ack": "D24", "train-update-apply": "D24", "train-update-check": "D24", "train-update-status": "D24", "trash-item": "D09", "unwatch-file": "D09", "unwatch-workspace": "D19", "viewer-close": "D23", "viewer-request-load": "D23", "watch-file": "D09", "watch-workspace": "D19", "window-close": "D23", "window-is-maximized": "D23", "window-maximize": "D23", "window-minimize": "D23", "write-file": "D09", "write-file-binary": "D09", "write-file-if-unchanged": "D09",
}
IPC_EVENTS = {
    "window-theme-changed": "D23", "settings-changed": "D23", "start-drag": "D23",
    "train-update-available": "D24", "train-fallback-notice": "D24", "train-update-progress": "D24", "quick-chat-open-session": "D01", "quick-chat-shown": "D01", "auto-update-state": "D24", "file-changed": "D09", "workspace-changed": "D19", "show-skill-viewer": "D21", "open-settings-modal": "D23", "settings-switch-tab": "D23", "server-restarted": "D20", "browser-update": "D14", "viewer-closed": "D23", "window-maximized": "D23", "window-unmaximized": "D23",
}
SLASH_DOMAINS = {"stop": "D01", "new": "D02", "reset": "D02", "rc": "D17", "exitrc": "D17", "apply": "D18", "confirm": "D04", "reject": "D04", "compact": "D01", "fresh-compact": "D01", "loop": "D19"}
SLASH_EFFECTS = {
    "stop": ("中止当前回复", "活动回复停止；没有活动流时收到明确提示"),
    "new": ("开启新会话", "旧历史归档，新会话可继续；接管态会拒绝"),
    "reset": ("重置会话", "当前会话历史清除；接管态会拒绝"),
    "rc": ("从 Bridge 私聊接管桌面会话", "列出可接管会话，选择后进入接管态并通知桌面"),
    "exitrc": ("退出桌面会话接管", "接管或待选状态清空，桌面横幅撤销"),
    "apply": ("应用自动任务建议", "建议被创建为自动任务或返回明确失败"),
    "confirm": ("确认待处理请求", "指定请求获批并继续执行"),
    "reject": ("拒绝待处理请求", "指定请求被拒绝，不执行待批动作"),
    "compact": ("压缩会话上下文", "先显示进行中，再显示压缩结果或错误"),
    "fresh-compact": ("刷新提示词和记忆后压缩", "显示压缩前后用量或失败原因"),
    "loop": ("启动或管理会话循环任务", "看到循环状态、暂停、恢复或停止结果"),
}
IPC_GROUPS = {
    "get-server-port": "service-connection", "get-server-token": "service-connection", "app-ready": "window-readiness", "get-platform": "window-readiness",
    "get-auto-launch-status": "auto-launch", "set-auto-launch-enabled": "auto-launch", "get-keep-awake-status": "keep-awake", "set-keep-awake-enabled": "keep-awake",
    "observability-export:begin": "observability-export", "observability-export:write": "observability-export", "observability-export:end": "observability-export", "observability-export:abort": "observability-export",
    "watch-file": "file-watch", "unwatch-file": "file-watch", "watch-workspace": "workspace-watch", "unwatch-workspace": "workspace-watch",
    "quick-chat-show": "quick-chat-window", "quick-chat-hide": "quick-chat-window", "quick-chat-resize": "quick-chat-window", "quick-chat-open-session": "quick-chat-window",
    "speech-permission-status": "speech-permission", "speech-permission-request": "speech-permission",
    "train-update-status": "train-update", "train-update-check": "train-update", "train-update-apply": "train-update", "train-fallback-notice-ack": "train-update",
    "auto-update-check": "windows-auto-update", "auto-update-install": "windows-auto-update", "auto-update-state": "windows-auto-update", "auto-update-set-channel": "windows-auto-update",
    "get-pending-announcement": "release-announcement", "ack-announcement": "release-announcement",
    "keybindings:reload-global": "global-shortcut", "keybindings:test-register": "global-shortcut", "quick-chat-reload-shortcut": "global-shortcut", "quick-chat-shortcut-status": "global-shortcut",
    "viewer-request-load": "detached-viewer", "viewer-close": "detached-viewer", "spawn-viewer": "detached-viewer",
    "read-file": "file-edit", "write-file": "file-edit", "read-file-snapshot": "file-edit", "write-file-if-unchanged": "file-edit", "write-file-binary": "file-edit", "copy-file": "file-edit", "read-file-base64": "file-edit", "run-edit-command": "file-edit",
    "window-minimize": "window-controls", "window-maximize": "window-controls", "window-close": "window-controls", "window-is-maximized": "window-controls",
    "browser-go-back": "browser-navigation", "browser-go-forward": "browser-navigation", "browser-reload": "browser-navigation", "browser-new-tab": "browser-tabs", "browser-switch-tab": "browser-tabs", "browser-close-tab": "browser-tabs",
    "select-folder": "file-picker", "select-files": "file-picker", "open-folder": "file-open", "open-file": "file-open", "show-in-finder": "file-open",
    "skill-viewer-list-files": "skill-viewer", "skill-viewer-read-file": "skill-viewer", "open-skill-viewer": "skill-viewer",
}
FILE_EDIT_SPLITS = {
    "read-file": "file-read", "read-file-snapshot": "file-read", "read-file-base64": "file-read",
    "write-file": "file-write-legacy", "write-file-binary": "file-write-binary",
    "copy-file": "file-copy", "run-edit-command": "edit-command",
}
IPC_EFFECTS = {
    "service-connection": ("连接本地服务", "桌面窗口获得本机服务地址和会话令牌，随后建立业务连接"),
    "window-readiness": ("初始化桌面窗口", "主窗口完成就绪通知并识别当前系统"),
    "auto-launch": ("设置开机启动", "开关状态写入系统登录项，重新打开设置可读到结果"),
    "keep-awake": ("设置保持唤醒", "操作系统阻止休眠的状态与设置开关一致"),
    "observability-export": ("导出模型观测", "保存对话框选择的文件接收分块内容；取消时中止导出"),
    "file-watch": ("监听预览文件", "文件变化后预览刷新，离开时取消监听"),
    "workspace-watch": ("监听工作区", "书桌文件树收到变化通知，关闭时释放监听"),
    "quick-chat-window": ("打开或收起快捷聊天", "快捷窗口改变尺寸和可见状态，选中会话可返回主窗口"),
    "speech-permission": ("检查或申请语音权限", "显示系统语音授权状态，用户操作后给出授权结果"),
    "train-update": ("检查并应用更新列车", "显示可用版本与进度；应用后重启，失败时保留回退提示"),
    "windows-auto-update": ("管理 Windows 自动更新", "显示通道与下载状态，用户选择安装时启动更新"),
    "release-announcement": ("阅读版本公告", "新版本公告显示一次，确认后不再重复弹出"),
    "global-shortcut": ("设置全局快捷键", "试注册反馈冲突，保存后快捷聊天快捷键生效"),
    "detached-viewer": ("打开独立文件预览", "派生窗口载入只读文件副本，关闭后释放窗口"),
    "file-edit": ("按版本保存工作区文本", "版本匹配时写入新文本和版本；版本已变时返回冲突且不覆盖"),
    "file-read": ("读取工作区文件", "文本、快照或 base64 按请求返回；无内容、过大或失败时明确空值或错误"),
    "file-write-legacy": ("通过旧桥写入文本", "直接写入指定文本；此入口没有版本冲突保护"),
    "file-write-binary": ("写入二进制文件", "二进制内容写入目标文件；失败时不宣称产物有效"),
    "file-copy": ("复制本地文件", "目标路径出现真实副本；源文件保留"),
    "edit-command": ("执行编辑器原生命令", "按允许的命令修改编辑器或剪贴板状态；不开放任意系统命令"),
    "window-controls": ("控制桌面窗口", "窗口最小化、最大化或关闭，标题栏同步状态"),
    "browser-navigation": ("在可见浏览器中导航", "当前标签前进、后退或刷新，页面与状态同步"),
    "browser-tabs": ("管理可见浏览器标签", "新增、切换或关闭标签，浏览器状态同步"),
    "file-picker": ("通过系统对话框选择文件", "取得用户实际选中的文件或目录，取消时不导入"),
    "file-open": ("在系统中打开文件", "文件或目录在系统应用/Finder 中打开"),
    "skill-viewer": ("预览技能文件", "弹出技能查看器并显示所选文件内容"),
    "open-settings-modal": ("从宿主打开设置弹窗", "主界面切换到指定设置页"),
    "settings-changed": ("同步跨窗口设置", "主窗口、设置窗口与浏览器查看器收到同一设置变更"),
    "settings-switch-tab": ("切换设置页", "设置窗口跳到宿主指定的标签"),
    "start-drag": ("拖出本地文件", "系统拖动携带真实文件，可交给其他应用"),
    "window-theme-changed": ("同步窗口主题", "主进程更新窗口外观以匹配当前主题"),
    "app:restart": ("重启桌面应用", "窗口关闭并重新启动应用"),
    "browser-emergency-stop": ("紧急停止浏览器", "当前浏览器会话停止自动操作，用户仍可查看窗口"),
    "close-browser-viewer": ("关闭可见浏览器窗口", "独立浏览器窗口关闭"),
    "debug-open-onboarding": ("打开引导调试界面", "调试入口显示首次使用引导"),
    "debug-open-onboarding-preview": ("预览引导界面", "预览窗口显示引导流程而不改完成状态"),
    "get-app-version": ("查看应用版本", "关于页显示当前安装版本"),
    "get-avatar-path": ("加载用户或 Agent 头像", "启动/引导界面显示已设置的头像"),
    "get-splash-info": ("加载启动画面资料", "启动画面显示当前 Agent 与头像信息"),
    "get-update-digest-history": ("查看更新历史", "关于页列出已知版本公告和变更摘要"),
    "onboarding-complete": ("完成首次使用引导", "引导完成标记保存，随后进入主界面"),
    "open-browser-viewer": ("打开可见浏览器", "独立浏览器窗口显示当前会话的网页"),
    "open-external": ("在系统浏览器打开链接", "系统默认浏览器打开所选外部地址"),
    "open-settings": ("打开设置窗口", "宿主创建或聚焦设置窗口并定位标签"),
    "read-docx-html": ("预览 Word 文件", "文件转换为可阅读的 HTML 预览"),
    "read-xlsx-html": ("预览表格文件", "工作簿转换为可阅读的 HTML 预览"),
    "release-check-latest": ("检查 GitHub 更新", "关于页显示可用新版或已是最新"),
    "reload-main-window": ("重新加载主窗口", "主界面重载并读取最新设置"),
    "screenshot-render": ("截取桌面内容", "截图产物返回聊天或文件预览"),
    "select-skill": ("选择技能目录", "系统选择框返回用户选定的技能路径"),
    "show-notification": ("显示系统通知", "系统通知展示消息和对应 Agent 头像"),
    "trash-item": ("移入系统废纸篓", "所选文件从原位置移走，可在系统废纸篓找回"),
}
# 这些投影只是同一可见行为的另一入口；预览有自己的只读结果，不能归入编辑。
UI_PROJECTION_OWNER = {"native-file-drag": "desktop-behavior:start-drag", "system-notification": "desktop-behavior:show-notification", "browser-visible": "desktop-behavior:open-browser-viewer", "quick-chat": "desktop-behavior:quick-chat-window", "observability-export-save": "desktop-behavior:observability-export"}
UI_PROJECTION_REASONS = {
    "native-file-drag": "同一次系统拖放的界面触发与宿主事件",
    "system-notification": "同一次系统通知的界面请求与宿主显示",
    "browser-visible": "同一个可见浏览器窗口的打开与界面投影",
    "quick-chat": "同一个快捷聊天窗口的触发与界面呈现；消息发送另属聊天入口",
    "observability-export-save": "同一次观测导出的界面保存动作与分块宿主写入",
    "file-preview": "独立只读预览；读取快照是共享底层能力，编辑写回另列",
}
SPECIAL_TASKS = {"ui-behavior:file-preview": ["R04-T04", "R06-T05", "R09-T05"]}
PREVIEW_ACCEPTANCE = ["R09-A09", "R09-A10"]
ROUTE_TASK_OVERRIDES = {
    "git-environment": ["R07-T07", "R07-T12"],
    "mcp": ["R04-T02", "R04-T07"],
    "access": ["R02-T03", "R07-T09"],
    "character-cards": ["R07-T08"],
    "cards": ["R07-T08"],
    "knowledge": ["R06-T07"],
}
CONFIRM_ROUTE = "route:confirm:POST:/confirm/:confirmId"
CONFIRM_EFFECTS = {
    "confirmed": ("批准待处理请求", "授权范围检查通过后待处理动作获批，前端确认卡与流事件显示 confirmed", [
        "server/routes/confirm.ts:14", "server/routes/confirm.ts:37", "desktop/src/react/components/chat/SettingsConfirmCard.tsx:98", "desktop/src/react/services/ws-message-handler.ts:1441"]),
    "rejected": ("拒绝待处理请求", "授权范围检查通过后待处理动作被拒，前端确认卡与流事件显示 rejected", [
        "server/routes/confirm.ts:14", "server/routes/confirm.ts:37", "desktop/src/react/components/chat/SettingsConfirmCard.tsx:109", "desktop/src/react/services/ws-message-handler.ts:1441"]),
}
REQUIRED_INTERNAL = frozenset({
    ("server-index", "/api/health"), ("server-index", "/api/log"), ("server-index", "/api/shutdown"),
    ("config", "/memories/health"),
    ("mcp", "/oauth/callback"), ("mcp", "/oauth/poll/:sessionId"),
    ("auth", "/auth/oauth/callback"), ("auth", "/auth/oauth/poll/:sessionId"),
})
IPC_EVENT_GROUPS = {"train-update-available": "train-update", "train-fallback-notice": "train-update", "train-update-progress": "train-update", "auto-update-state": "windows-auto-update", "quick-chat-open-session": "quick-chat-window", "quick-chat-shown": "quick-chat-window", "file-changed": "file-watch", "workspace-changed": "workspace-watch", "viewer-closed": "detached-viewer", "window-maximized": "window-controls", "window-unmaximized": "window-controls", "browser-update": "browser-tabs", "show-skill-viewer": "skill-viewer", "server-restarted": "service-connection"}
IPC_STORES = {
    "service-connection": "本地服务端口和短期令牌（内存）", "window-readiness": "窗口运行态（内存）", "auto-launch": "系统登录项", "keep-awake": "系统唤醒阻止状态（运行态）", "observability-export": "观测 SQLite；用户选定导出文件", "file-watch": "文件系统监听句柄和工作区文件", "workspace-watch": "工作区监听句柄和文件树", "quick-chat-window": "悬浮窗口状态；session JSONL", "speech-permission": "系统语音权限状态", "train-update": "更新状态；release digest；已下载更新", "windows-auto-update": "Windows 更新状态和通道偏好", "release-announcement": "本机已读版本标记；release digest", "global-shortcut": "快捷键偏好；系统注册状态", "detached-viewer": "窗口状态；只读文件副本", "file-edit": "授权工作区文件；文件版本指纹", "window-controls": "窗口运行态", "browser-navigation": "浏览器 session/profile 和当前页", "browser-tabs": "浏览器 session/profile 和标签", "file-picker": "用户选择的本地路径", "file-open": "本地文件或目录", "skill-viewer": "已安装技能目录及文件", "start-drag": "实际文件路径；系统拖动会话", "settings-changed": "设置状态；窗口间广播", "show-notification": "通知运行态；Agent 头像", "screenshot-render": "屏幕捕获图像", "trash-item": "工作区文件；系统废纸篓", "read-docx-html": "本地 DOCX 文件；临时 HTML", "read-xlsx-html": "本地 XLSX 文件；临时 HTML", "get-update-digest-history": "release-digest.v1/v2.json", "release-check-latest": "GitHub Releases 状态；应用版本", "onboarding-complete": "首次引导完成标记", "get-app-version": "安装包版本", "get-avatar-path": "Agent/user 头像文件", "get-splash-info": "Agent 配置与头像文件", "open-browser-viewer": "浏览器 session/profile 和窗口", "open-settings": "设置窗口状态", "open-settings-modal": "设置弹窗状态", "settings-switch-tab": "设置窗口标签", "window-theme-changed": "主题偏好；窗口状态", "app:restart": "应用进程状态", "browser-emergency-stop": "浏览器会话运行态", "close-browser-viewer": "浏览器窗口状态", "reload-main-window": "主窗口状态", "select-skill": "用户选择的技能路径", "open-external": "系统默认浏览器会话", "debug-open-onboarding": "引导窗口状态", "debug-open-onboarding-preview": "引导预览窗口状态",
}

ROUTE_OVERRIDES = {
    ("sessions", "/browser/"): "D14", ("config", "/search/"): "D14", ("config", "/config"): "D10",
    ("config", "/user-profile"): "D23", ("desk", "/desk/beautify/"): "D16", ("desk", "/desk/cron"): "D18", ("desk", "/desk/heartbeat"): "D18", ("desk", "/desk/install-skill"): "D21", ("desk", "/desk/delete-skill"): "D21", ("desk", "/desk/skills"): "D21",
    ("preferences", "/preferences/computer-use"): "D15", ("preferences", "/preferences/browser"): "D14", ("preferences", "/preferences/models"): "D10", ("preferences", "/preferences/goal"): "D19", ("preferences", "/preferences/autolearn"): "D07", ("preferences", "/preferences/session-permission"): "D04", ("preferences", "/preferences/quick-chat"): "D01",
    ("agents", "/agents/:id/skills"): "D21", ("agents", "/agents/:id/tenets"): "D07", ("agents", "/agents/:id/experience"): "D07", ("agents", "/agents/:id/pinned"): "D07",
    ("experiments", "/experiments/memory/"): "D07",
    ("media", "/media/asr/"): "D15", ("bridge", "/bridge/media/"): "D09", ("mobile-workbench", "/workbench/"): "D19", ("mobile-workbench", "/mobile/workbench/"): "D19",
    ("git-environment", "/git/"): "D19", ("html-preview", "/preview/"): "D09", ("html-preview", "/api/preview/"): "D09",
}

ACTION = {"GET": "查看", "POST": "执行", "PUT": "设置", "PATCH": "修改", "DELETE": "删除", "ON": "读取"}
RESULT = {"GET": "看到对应列表、详情或资源", "POST": "看到操作结果或新建记录", "PUT": "看到设置后的状态", "PATCH": "看到更新后的状态", "DELETE": "看到删除或撤销结果", "ON": "收到资源响应"}

BEHAVIOR_TITLES = {
    ("sessions", "/sessions"): "查看会话列表", ("sessions", "/sessions/search"): "搜索会话", ("sessions", "/sessions/pin"): "置顶或取消置顶会话", ("sessions", "/sessions/archive"): "归档会话", ("sessions", "/sessions/restore"): "恢复归档会话", ("sessions", "/sessions/archived/delete"): "删除归档会话", ("sessions", "/sessions/fork"): "从会话分支", ("sessions", "/sessions/turns/retry"): "重试一轮消息", ("sessions", "/sessions/turns/rollback-preview"): "预览会话回退", ("sessions", "/sessions/workspace-rollback"): "回退工作区", ("sessions", "/sessions/messages"): "阅读会话历史", ("sessions", "/sessions/new"): "新建会话", ("sessions", "/sessions/rename"): "重命名会话",
    ("knowledge", "/knowledge/notebooks"): "查看或创建知识笔记本", ("knowledge", "/knowledge/notebooks/:id/sources"): "查看或添加知识来源", ("knowledge", "/knowledge/notebooks/:id/import-directory"): "导入目录资料", ("knowledge", "/knowledge/citations/:citationId"): "打开知识引用", ("knowledge", "/knowledge/ingestion"): "查看知识导入进度",
    ("server-index", "/api/health"): "查看服务连接状态", ("server-index", "/api/log"): "记录界面诊断", ("server-index", "/api/shutdown"): "关闭本地服务",
    ("mcp", "/connectors"): "管理 MCP 连接器", ("mcp", "/state"): "查看 MCP 连接状态",
    ("providers", "/providers/fetch-models"): "发现供应商模型", ("models", "/models/switch"): "切换对话模型",
    ("media", "/media/image/generate"): "生成图片", ("media", "/media/video/generate"): "生成视频", ("media", "/media/speech/generate"): "生成语音",
    ("file-history", "/file-history/versions"): "查看文件历史版本", ("file-history", "/file-history/restore"): "恢复文件版本",
    ("conversation-map", "/conversation-map/layout"): "查看或调整会话地图布局", ("devices", "/devices/pairing-sessions"): "配对设备",
}

PATH_WORDS = {
    "access": "访问", "account": "账号", "actions": "操作", "activities": "活动", "agent": "Agent", "agents": "Agent", "ai-commit-message": "提交说明建议", "appearance": "外观", "approve": "批准", "approvals": "批准", "archived": "已归档", "assets": "素材", "asr": "语音识别", "attachments": "附件", "auth": "登录", "authorized-folders": "授权目录", "automation": "自动化", "avatar": "头像", "batch": "批次", "beautify": "美化", "blobs": "内容块", "bootstrap": "初始化", "branches": "分支", "bridge": "Bridge", "browser": "浏览器", "bundles": "技能包", "calls": "调用", "cancel": "取消", "capabilities": "能力", "cards": "角色卡", "channels": "频道", "chat": "聊天", "checkpoints": "检查点", "checkout": "切换分支", "citations": "引用", "clear-cookies": "清除 Cookie", "commands": "命令", "compact": "压缩", "compiled": "整理后", "computer-use": "电脑操作", "config": "配置", "confirm": "确认", "connectors": "连接器", "content": "内容", "conversation-map": "会话地图", "conversations": "对话", "cover": "封面", "create-branch": "新建分支", "credentials": "凭证", "cron": "定时任务", "data": "数据", "delete": "删除", "desk": "工作台", "devices": "设备", "diagnostics": "诊断", "diary": "日记", "diff": "差异", "discard": "丢弃", "dm": "私信", "download": "下载", "dream": "梦境", "env-deps": "环境依赖", "events": "事件", "experiments": "实验开关", "export": "导出", "external-paths": "外部路径", "fetch": "拉取", "fetch-models": "发现模型", "file-diff": "文件差异", "file-history": "文件历史", "files": "文件", "find": "查找", "folders": "文件夹", "fork": "分支", "fresh-compact": "新鲜压缩", "fs": "文件系统", "generated": "生成文件", "git": "Git", "goal": "目标", "health": "健康状态", "heartbeat": "心跳", "history-overview": "历史概览", "html": "HTML", "image": "图片", "import": "导入", "import-directory": "导入目录", "ingestion": "导入进度", "input-drafts": "输入草稿", "interface": "界面", "jian": "便笺", "knowledge": "知识库", "layout": "布局", "list": "列表", "log": "日志", "logs": "日志", "maintenance": "维护", "map": "地图", "media": "媒体", "members": "成员", "memories": "记忆", "memory": "记忆", "messages": "消息", "mobile": "移动端", "model-observability": "模型观测", "models": "模型", "move": "移动", "mcp": "MCP", "network": "网络", "new": "新建", "notebooks": "笔记本", "notifications": "通知", "oauth": "OAuth", "observation": "观察", "open": "打开", "order": "顺序", "parse-artifacts": "解析产物", "password": "密码", "payloads": "请求详情", "permissions": "权限", "pin": "置顶", "pin-order": "置顶顺序", "plans": "方案", "preferences": "偏好", "preview": "预览", "profile": "资料", "projects": "项目", "providers": "供应商", "pull": "拉取", "push": "推送", "query": "查询", "quick-chat": "快捷聊天", "read": "读取", "reingest": "重新入库", "rename": "重命名", "resource-io": "资源操作", "resources": "资源", "restore": "恢复", "retry": "重试", "revisions": "修订", "rollback-preview": "回退预览", "search": "搜索", "security": "安全", "server": "服务", "session": "会话", "sessions": "会话", "settings": "设置", "sharing": "分享", "skills": "技能", "snapshot": "快照", "sources": "来源", "speech": "语音", "stage": "暂存", "stashes": "暂存项", "status": "状态", "stop": "停止", "studio": "工作室", "subscriptions": "订阅", "summary": "摘要", "switch": "切换", "tasks": "任务", "test": "测试", "thinking-level": "思考强度", "ticket": "票据", "todo": "待办", "todos": "待办", "traces": "轨迹", "translate": "翻译", "trash": "移入废纸篓", "turns": "轮次", "unstage": "取消暂存", "unstash": "取回暂存", "update": "更新", "upload": "上传", "usage": "用量", "user-profile": "用户资料", "versions": "版本", "video": "视频", "watch": "监听", "web-auth": "网页登录", "workbench": "工作台", "workspace-rollback": "工作区回退", "workspaces": "工作区", "write": "写入", "ws-ticket": "连接票据",
}

PATH_WORDS.update({
    "abort": "终止", "agent-activities": "Agent活动", "agent-phone-settings": "Agent通话设置", "agent-phone-tool-mode": "Agent通话工具模式", "agents-md": "Agent说明", "aggregate": "汇总", "all": "全部", "amend": "修订提交", "api-key": "API密钥", "app-tools": "应用工具", "apply": "应用", "apps": "应用", "archive": "归档", "autolearn": "自动学习", "auxiliary-vision": "辅助视觉", "blocks": "内容块", "bulk": "批量", "cache-snapshot-reflection": "缓存反思", "call": "调用", "callback": "回调", "character-cards": "角色卡", "chunks": "分块", "cleanup": "清理", "cleanup-preview": "清理预览", "close-session": "关闭会话", "commit": "提交", "complete": "完成", "continue-deleted-agent": "续接已删除Agent", "custom-models": "自定义模型", "days": "每日", "decide": "决定", "default-workspace": "默认工作区", "defer": "按需调用", "delete-skill": "删除技能", "desktop-credentials": "桌面凭证", "discovered-models": "发现的模型", "dismiss": "关闭", "docx-html": "文档预览", "enabled": "启用状态", "experience": "经验", "facts": "事实", "feedback": "反馈", "generate": "生成", "hardware-acceleration": "硬件加速", "identity": "身份", "install": "安装", "install-skill": "安装技能", "ishiki": "人格意识", "keybindings": "快捷键", "latest-user-message": "最近用户消息", "launch": "启动", "legacy-gpu-safe-mode": "旧版GPU安全模式", "llm": "语言模型", "log-stats": "日志统计", "login": "登录", "logout": "退出登录", "longterm": "长期", "mobile-credentials": "移动端凭证", "mobile-qr.svg": "移动端二维码", "new-detached": "新建独立会话", "open-session": "打开会话", "owner": "主人", "pairing-sessions": "配对会话", "path": "路径", "pinned": "置顶", "plan": "计划", "plan-mode": "计划模式", "poll": "轮询", "preset": "预设", "primary": "主Agent", "promote": "转为正式", "prompt-snapshot": "提示词快照", "public-agents-md": "公开Agent说明", "public-ishiki": "公开人格意识", "qrcode": "二维码", "qrcode-status": "二维码状态", "read-base64": "二进制读取", "recent": "最近", "refresh": "刷新", "refresh-tools": "刷新工具", "reject": "拒绝", "reload": "重载", "reorder": "排序", "replay": "重放", "request-permissions": "请求权限", "reset": "重置", "revoke": "撤销", "runs": "运行", "search-files": "搜索文件", "send-media": "发送媒体", "servers": "服务连接", "session-assignment": "会话归属", "session-collab": "会话协作", "session-permission-default": "会话默认权限", "session-permission-mode": "会话权限模式", "session-permissions": "会话授权", "session-projects": "会话项目", "session-states": "会话状态", "session-thinking-level": "会话思考强度", "set": "设置", "setup-complete": "设置完成", "shutdown": "关闭服务", "sidebar-ui": "侧栏界面", "skill-bundles": "技能包", "snapshots": "快照", "speech-recognition": "语音识别", "start": "启动", "stash": "暂存", "stat": "文件状态", "state": "状态", "storage": "存储", "subscribe": "订阅", "summarize": "总结", "sweep-orphaned-workspaces": "清理孤立工作区", "system": "系统", "task": "任务", "tenets": "信条", "today": "今日", "toggle": "切换", "transcribe": "转写", "upload-blob": "上传数据块", "user-edit": "用户编辑", "verify": "验证", "watch-diagnostics": "监听诊断", "wechat": "微信", "week": "每周", "workspace-disposal": "工作区处置", "workspace-ui-state": "工作区界面状态", "worktree-create": "创建工作树", "worktree-info": "工作树信息", "worktrees": "工作树", "write-expected-version": "按版本写入", "ws": "实时连接", "xlsx-html": "表格预览",
})


def route_label(route: str, path: str, domain: str) -> str:
    specific = BEHAVIOR_TITLES.get((route, path))
    if specific:
        return specific
    segments = [segment for segment in path.strip("/").split("/") if segment and not segment.startswith(":") and segment not in {"api", "id"} and not segment.startswith("*") and "[" not in segment]
    if not segments:
        return DOMAINS[domain][0]
    unknown = [segment for segment in segments if segment not in PATH_WORDS]
    if unknown:
        raise ValueError(f"路由行为尚未命名: {route} {path} {unknown}")
    return "".join(PATH_WORDS.get(segment, segment) for segment in segments)


def lines(path: str) -> list[str]:
    return (ROOT / path).read_text(encoding="utf-8").splitlines()


def ref(path: str, line: int) -> str:
    return f"{path}:{line}"


def slug(s: str) -> str:
    result = re.sub(r"[^A-Za-z0-9]+", "-", s).strip("-").upper()
    return result[:88]


def make_id(domain: str, kind: str, key: str) -> str:
    # 内容哈希避免截断后碰撞；同一注册身份重跑得到同一 F-ID。
    return f"F-{domain}-{kind}-{slug(key)[:48]}-{hashlib.sha256(key.encode()).hexdigest()[:6].upper()}"


def route_domain(route: str, path: str) -> str:
    for (r, prefix), domain in sorted(ROUTE_OVERRIDES.items(), key=lambda item: -len(item[0][1])):
        if route == r and path.startswith(prefix):
            return domain
    return ROUTE_DOMAINS[route]


def behavior_key(item: dict) -> str:
    if item["kind"] != "route":
        return item["entry_id"]
    _, route, method, path = item["entry_id"].split(":", 3)
    # 同一处理函数的旧地址沿用首个现行地址。注册地址仍各自留在 coverage。
    if route == "mcp":
        path = path.replace("/servers", "/connectors")
        if path == "/settings/enabled":
            path = "/enabled"
    elif route == "agents":
        path = path.replace("/public-ishiki", "/public-agents-md").replace("/ishiki", "/agents-md")
    elif route == "mobile-workbench":
        path = path.replace("/mobile/workbench/", "/workbench/")
    # 行为身份独立于注册地址；GET/HEAD 经核对是同一资源读取，其余方法有不同效果。
    effect = "READ" if method in {"GET", "HEAD"} else method
    return f"behavior:{route}:{path}:{effect}"


INTERNAL_ROUTE_OWNER = {
    ("server-index", "/api/health"): "ui-behavior:service-connectivity",
    ("server-index", "/api/log"): "ui-behavior:service-connectivity",
    ("server-index", "/api/shutdown"): "ui-behavior:service-connectivity",
    ("config", "/memories/health"): "behavior:config:/memories:READ",
    ("mcp", "/oauth/callback"): "behavior:mcp:/connectors/:id/oauth/start:POST",
    ("mcp", "/oauth/poll/:sessionId"): "behavior:mcp:/connectors/:id/oauth/start:POST",
    ("auth", "/auth/oauth/callback"): "behavior:auth:/auth/oauth/start:POST",
    ("auth", "/auth/oauth/poll/:sessionId"): "behavior:auth:/auth/oauth/start:POST",
}

ROUTE_EFFECTS = {
    ("model-observability", "/model-observability/health", "READ"): ("查看模型观测服务状态", "返回录制与查询可用状态；用量页据此显示可查询内容，权限或网络失败显示错误提示而非空用量"),
    ("knowledge", "/knowledge/notebooks", "READ"): ("查看知识笔记本列表", "返回当前工作空间的笔记本、有效分块配置和导入摘要"),
    ("knowledge", "/knowledge/notebooks", "POST"): ("创建知识笔记本", "校验名称并写入当前工作空间知识库，返回新笔记本或错误"),
    ("knowledge", "/knowledge/notebooks/:id", "READ"): ("查看知识笔记本", "返回当前笔记本资料；失败时返回具体错误"),
    ("knowledge", "/knowledge/notebooks/:id", "PATCH"): ("重命名知识笔记本", "名称写入知识库并返回更新后的笔记本；失败时不报告成功"),
    ("knowledge", "/knowledge/notebooks/:id", "DELETE"): ("删除知识笔记本", "删除笔记本及其来源并返回删除结果；引用保护或失败时返回错误"),
    ("access", "/access/account/password", "PUT"): ("设置本地账号密码", "仅本地主人可设置密码；更新账号并记录安全审计，失败时返回错误"),
    ("access", "/access/account/password", "DELETE"): ("移除本地账号密码", "仅本地主人可清除密码；更新账号并记录安全审计，失败时返回错误"),
    ("mcp", "/connectors/:id", "PUT"): ("修改 MCP 连接器", "连接器配置更新，返回更新结果或错误"),
    ("mcp", "/connectors/:id", "DELETE"): ("删除 MCP 连接器", "连接器配置及运行状态移除，返回删除结果或错误"),
    ("sessions", "/sessions/workspace-rollback", "READ"): ("查看文件回退开关", "返回当前 enabled 状态；此读取不会回退文件"),
    ("sessions", "/sessions/workspace-rollback", "PUT"): ("设置文件回退开关", "将 enabled 布尔偏好写入并返回新状态；此接口本身不会回退文件"),
    ("git-environment", "/git/push", "POST"): ("推送当前分支", "实际执行 git push；无上游时尝试 push -u，界面显示成功、无远端、无待推送内容或失败"),
    ("git-environment", "/git/pull", "POST"): ("拉取远端分支", "实际获取远端并在允许时更新本地分支/工作树；界面显示拉取结果、冲突或失败"),
    ("git-environment", "/git/fetch", "POST"): ("刷新远端 Git 状态", "实际执行 git fetch，只更新远端跟踪信息而不合并或改工作树；面板显示结果或失败"),
    ("providers", "/providers/fetch-models", "POST"): ("发现供应商可用模型", "经 providers.manage 和凭证边界探测远端模型目录；API key 远端失败明确报错，OAuth 目录按现行规则回落"),
    ("knowledge", "/knowledge/notebooks/:id/import-directory", "POST"): ("导入目录到知识笔记本", "登记来源并启动受控入库；返回导入状态或权限、解析错误"),
    ("knowledge", "/knowledge/notebooks/:id/sources", "READ"): ("查看笔记本来源", "返回当前来源列表，不导入新资料"),
    ("knowledge", "/knowledge/notebooks/:id/sources", "POST"): ("添加笔记本来源", "登记新来源并返回入库任务或明确错误"),
    ("config", "/config/workspaces/recent", "DELETE"): ("移除一项最近工作区记录", "从当前 Agent 的 cwd_history 移除指定路径并返回新列表；不删除目录"),
    ("config", "/config/workspaces/recent/all", "DELETE"): ("清空最近工作区记录", "清空当前 Agent 的 cwd_history 并返回空列表；不删除目录"),
    ("config", "/memories/compiled", "DELETE"): ("清除整理后的记忆产物", "清理编译记忆与总结来源并留下重置标记；原始事实库不按此接口清空"),
    ("config", "/memories", "DELETE"): ("清除当前 Agent 全部记忆", "清空事实库与整理产物并留下重置标记；失败返回错误"),
    ("config", "/memories/import", "POST"): ("导入记忆事实", "将兼容 v1/v2 的事实写入指定 Agent 记忆库，返回实际导入条数或错误"),
    ("file-history", "/file-history/restore", "POST"): ("恢复文件历史版本", "通过 ResourceIO 写回工作区文件、再记录恢复快照；返回路径或错误"),
    ("git-environment", "/git/discard", "POST"): ("丢弃 Git 已跟踪文件改动", "按所选路径还原工作树，未给路径时还原全部已跟踪改动；返回结果或错误"),
    ("preferences", "/preferences/computer-use/approvals", "DELETE"): ("撤销电脑操作应用批准", "更新应用批准设置并发送设置变更事件；不支持的平台返回明确错误"),
    ("preferences", "/preferences/computer-use/approvals", "POST"): ("批准电脑操作应用", "更新应用批准设置并发送设置变更事件；不支持的平台返回明确错误"),
    ("preferences", "/preferences/computer-use/request-permissions", "POST"): ("申请电脑操作系统权限", "调用系统权限申请并返回结果；Linux Preview 返回不支持错误"),
    ("preferences", "/preferences/browser/clear-cookies", "POST"): ("清除内置浏览器站点资料", "清除 Cookie 与站点数据后返回成功，失败时返回错误"),
    ("sessions", "/sessions/cleanup", "POST"): ("清理过期归档会话", "按期限永久删除过期归档文件并清理关联生命周期与标题，返回删除数量或错误"),
    ("sessions", "/sessions/archived/delete", "POST"): ("永久删除归档会话", "清理会话生命周期与归档文件，返回删除结果或错误；不能通过恢复接口找回"),
    ("sessions", "/sessions/continue-deleted-agent", "POST"): ("续接已删除 Agent 的会话", "校验会话范围后生成可续接的新会话身份并返回路径或错误"),
    ("sessions", "/sessions/turns/rollback-preview", "POST"): ("预览会话轮次回退", "返回拟撤销的轮次与工作区影响；预览本身不写回文件"),
    ("session-collab", "/session-collab/reject", "POST"): ("拒绝会话协作请求", "请求被拒绝并返回处理结果，不建立协作关系"),
    ("character-cards", "/character-cards/import", "POST"): ("提交角色卡导入方案", "用预览令牌提交导入计划，创建 Agent，可选导入记忆与技能，并发出变更事件"),
    ("auth", "/auth/oauth/logout", "POST"): ("退出模型 OAuth 登录", "删除指定供应商凭证、清空认证缓存并刷新模型列表，返回成功或错误"),
    ("web-auth", "/web-auth/logout", "POST"): ("退出网页登录", "撤销当前 Web 会话并清除会话 Cookie，返回成功"),
    ("model-observability", "/model-observability/maintenance/delete", "POST"): ("按筛选条件清理模型观测数据", "调用观测存储删除并返回实际结果；无存储、筛选无效或查询失败均明确报错"),
    ("desk", "/desk/delete-skill", "POST"): ("删除工作区技能目录", "只允许当前工作区已知技能目录；递归删除后重新同步目录，失败返回错误"),
    ("experiments", "/experiments/memory/cache-snapshot-reflection/observation", "DELETE"): ("删除记忆缓存观察记录", "校验 settings.write 范围后删除当前 Agent 观察记录，返回 deleted 状态"),
    ("mcp", "/session-permissions", "POST"): ("批准当前会话的一次 MCP 工具能力", "校验会话与 sessions.write 范围后，仅在本次会话内放行指定 capability；不持久化"),
    ("preferences", "/preferences/session-permission-default", "READ"): ("查看新会话默认权限", "返回当前 permissionMode，未设置时为 ASK"),
    ("preferences", "/preferences/session-permission-default", "PUT"): ("设置新会话默认权限", "校验模式并写入偏好，返回保存后的 permissionMode 或错误"),
    ("server-index", "/api/session-permission-mode", "READ"): ("查看当前会话权限模式", "返回 mode、accessMode 与 defaultMode，不改变权限"),
    ("server-index", "/api/session-permission-mode", "POST"): ("切换指定作用域的会话权限模式", "按当前会话、待新建会话、指定会话或全局作用域设置模式，返回实际模式或冲突错误"),
    ("sessions", "/sessions/restore", "POST"): ("恢复归档会话", "经路径和冲突校验后把归档文件及 sidecar 移回活跃目录并更新生命周期，返回新路径或错误"),
}

SEMANTIC_AUDIT_FILES = (
    "R00-T02_SEMANTIC_AUDIT_G1.json",
    "R00-T02_SEMANTIC_AUDIT_G2.json",
    "R00-T02_SEMANTIC_AUDIT_G3.json",
)
NONHTTP_AUDIT_FILES = (
    "R00-T02_NONHTTP_AUDIT_TOOLS.json",
    "R00-T02_NONHTTP_AUDIT_UI.json",
    "R00-T02_NONHTTP_AUDIT_CORE.json",
    "R00-T02_NONHTTP_AUDIT_SPLITS.json",
)
SOURCE_BRANCH_AUDIT_FILE = "R00-T02_SOURCE_BRANCH_AUDIT.json"
BRANCH_CONTRACT_FILE = "R00-T02_BRANCH_SEMANTIC_CONTRACT.json"
UI_ACTION_MATRIX_FILE = "R00-T02_UI_ACTION_MATRIX.json"
SOURCE_BRANCH_AUDIT_OVERRIDE = None
BASELINE_PENDING_COUNT = 382
BASELINE_PENDING_IDS_SHA256 = "24ed7883e7a643c72181f462bb1a4ac6492b1ebda56d8d6caa2e4b1f2a2b29d4"


@lru_cache(maxsize=1)
def semantic_reviews() -> dict[str, dict]:
    reviewed = {}
    for name in SEMANTIC_AUDIT_FILES:
        path = OUT / name
        if not path.exists():
            raise ValueError(f"缺少逐入口语义审查: {name}")
        report = json.loads(path.read_text(encoding="utf-8"))
        rows = report.get("reviews", report.get("entries"))
        if not isinstance(rows, list):
            raise ValueError(f"审查报告缺少逐项记录: {name}")
        for row in rows:
            key = row.get("entry_id")
            if not key or key in reviewed:
                raise ValueError(f"审查入口重复或缺失: {name} {key}")
            reviewed[key] = row
    return reviewed


@lru_cache(maxsize=1)
def nonhttp_reviews() -> dict[str, dict]:
    reviewed = {}
    for name in NONHTTP_AUDIT_FILES:
        path = OUT / name
        if not path.exists():
            raise ValueError(f"缺少非 HTTP 逐叶审查: {name}")
        report = json.loads(path.read_text(encoding="utf-8"))
        rows = report.get("reviews")
        if not isinstance(rows, list) or report.get("review_count", report.get("reviewed_count")) != len(rows):
            raise ValueError(f"非 HTTP 审查数量不符: {name}")
        for row in rows:
            key = row.get("original_feature_id")
            if not key or key in reviewed:
                raise ValueError(f"非 HTTP 审查 F-ID 重复或缺失: {name} {key}")
            reviewed[key] = row
    return reviewed


def source_file_sha256(source: str) -> str:
    return hashlib.sha256("\n".join(lines(source)).encode()).hexdigest()


def review_record_sha256(review: dict) -> str:
    return hashlib.sha256(json.dumps(review, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def validate_source_branch_audit(registries: list[dict], features: list[dict], coverage: dict) -> None:
    """所有生产登记走同一冻结源码/行为/场景门禁，特殊路由没有豁免。"""
    path = OUT / SOURCE_BRANCH_AUDIT_FILE
    if not path.exists():
        raise ValueError(f"缺少统一入口分支审查: {SOURCE_BRANCH_AUDIT_FILE}")
    report = SOURCE_BRANCH_AUDIT_OVERRIDE or json.loads(path.read_text(encoding="utf-8"))
    rows = report.get("reviews")
    if not isinstance(rows, list) or report.get("review_count") != len(rows):
        raise ValueError("统一入口分支审查数量不符")
    by_entry = {row.get("entry_id"): row for row in rows}
    active = {entry["entry_id"] for entry in registries}
    if len(by_entry) != len(rows) or set(by_entry) != active:
        raise ValueError(f"统一入口分支审查差集: 缺少={sorted(active-set(by_entry))}, 多出={sorted(set(by_entry)-active)}")
    mapped = {row["entry_id"]: row for row in coverage["registrations"]}
    raw_by_entry = {entry["entry_id"]: entry for entry in registries}
    scenarios = {feature["feature_id"]: feature["supplemental_acceptance_id"] for feature in features}
    feature_by_id = {feature["feature_id"]: feature for feature in features}
    ask_review = next((review for review in nonhttp_reviews().values() if review["entry_id"] == "tool:ask_user"), None)
    if ask_review is None:
        raise ValueError("ask_user 人工分支契约失去逐叶审查")
    contract = json.loads((OUT / BRANCH_CONTRACT_FILE).read_text(encoding="utf-8"))
    validate_ask_user_contract(contract, ask_review, ask_review["scenario"], ROOT, lines)
    ui_matrix = json.loads((OUT / UI_ACTION_MATRIX_FILE).read_text(encoding="utf-8"))
    matrix_pages = {page["entry_id"]: page for page in ui_matrix["pages"]}
    current_source_hashes = {}
    for entry_id, row in by_entry.items():
        registered = mapped[entry_id]
        expected_features = set(registered["feature_ids"])
        if set(row.get("feature_ids", [])) != expected_features:
            raise ValueError(f"统一入口行为归属变化，须复审: {entry_id}")
        if set(row.get("scenario_ids", [])) != {scenarios[fid] for fid in expected_features}:
            raise ValueError(f"统一入口逐叶场景变化，须复审: {entry_id}")
        expected_audits = {}
        for fid in expected_features:
            feature = feature_by_id[fid]
            audit = semantic_reviews().get(feature.get("semantic_review_entry_id")) if feature["kind"] == "route_behavior" else nonhttp_reviews().get(fid)
            if audit:
                expected_audits[fid] = review_record_sha256(audit)
        if row.get("audit_record_sha256") != expected_audits:
            raise ValueError(f"统一入口审查记录与源码签名/裁决未同步: {entry_id}")
        if row.get("decision") not in {"SPLIT", "SAME_EFFECT_WITH_VARIANTS", "INTERNAL_STEP", "UNREACHABLE", "SUPPORT", "VARIANT", "INDEPENDENT"}:
            raise ValueError(f"统一入口分支裁决未结案: {entry_id}")
        if not all(isinstance(row.get(key), str) and len(row[key].strip()) >= 4 for key in ("reason", "positive", "boundary")):
            raise ValueError(f"统一入口缺少正反结果与裁决依据: {entry_id}")
        for manual in row.get("manual_boundaries", []):
            if not all(isinstance(manual.get(key), str) and len(manual[key].strip()) >= 8
                       for key in ("callsite", "unknown", "reason", "review_conclusion", "recheck_trigger")):
                raise ValueError(f"动态边界 MANUAL 缺调用点、原因、人工结论或复审触发: {entry_id}")
        hashes = row.get("reviewed_source_sha256")
        if not isinstance(hashes, dict) or not hashes:
            raise ValueError(f"统一入口未冻结 handler/helper/消费者: {entry_id}")
        required_sources = {source.rsplit(":", 1)[0] for source in raw_by_entry[entry_id]["source_ref"]}
        for fid in expected_features:
            feature = feature_by_id[fid]
            required_sources.update(source.rsplit(":", 1)[0] for source in feature["source_ref"])
            audit = semantic_reviews().get(feature.get("semantic_review_entry_id")) if feature["kind"] == "route_behavior" else nonhttp_reviews().get(fid)
            if audit:
                required_sources.update(audit.get("reviewed_source_sha256", {}))
        if entry_id == "tool:ask_user":
            required_sources.update({"lib/confirm-store.ts", "server/routes/confirm.ts"})
        if raw_by_entry[entry_id]["kind"] == "ui":
            ui_feature = next((feature_by_id[fid] for fid in expected_features if feature_by_id[fid]["kind"] == "ui"), None)
            if ui_feature is None:
                raise ValueError(f"UI 登记缺页面叶: {entry_id}")
            closure, manual = ui_consumer_closure(entry_id, ui_feature["source_ref"], ROOT, lines)
            required_sources.update(closure)
            required_sources.update(matrix_pages[entry_id].get("consumer_closure", {}).get("files", []))
            required_sources.update(ref.rsplit(":", 1)[0]
                                    for action in matrix_pages[entry_id]["actions"]
                                    for ref in action["call_chain"])
            if manual and not row.get("manual_boundaries"):
                raise ValueError(f"UI 消费链动态边界未列 MANUAL: {entry_id}")
        if not required_sources <= set(hashes) or set(row.get("source_scope", [])) != set(hashes):
            raise ValueError(f"统一入口缺 handler/helper/最终消费者签名: {entry_id} {sorted(required_sources-set(hashes))[:8]}")
        for source, expected in hashes.items():
            if source not in current_source_hashes:
                current_source_hashes[source] = source_file_sha256(source)
            if current_source_hashes[source] != expected:
                raise ValueError(f"统一入口源码分支签名变化，须重新审查: {entry_id} {source}")


def validate_nonhttp_review(feature: dict, review: dict, all_feature_ids: set[str]) -> None:
    fid = feature["feature_id"]
    if review.get("entry_id") != feature["entry_id"] or review.get("kind") != feature["kind"]:
        raise ValueError(f"非 HTTP 审查身份不匹配: {fid}")
    if review.get("decision") not in {"INDEPENDENT", "SUPPORT", "VARIANT"}:
        raise ValueError(f"非 HTTP 叶无行为身份裁决: {fid}")
    supported = review.get("supported_feature_ids", [])
    if not isinstance(supported, list) or set(supported) - all_feature_ids or fid in supported:
        raise ValueError(f"非 HTTP 支持目标失效: {fid}")
    if review["decision"] == "SUPPORT" and not supported:
        raise ValueError(f"支持入口未指向被支持语义叶: {fid}")
    if review["decision"] == "VARIANT" and not supported and not review.get("variant_group"):
        raise ValueError(f"能力变体缺少支持语义叶或变体矩阵: {fid}")
    for key in ("user_action", "success_result", "refusal_or_boundary"):
        if not isinstance(review.get(key), str) or len(review[key].strip()) < 4:
            raise ValueError(f"非 HTTP 审查缺少具体动作或结果: {fid} {key}")
    if any(template in review["success_result"] for template in ("结果或产物", "工具结果进入会话", "看到页面内容")):
        raise ValueError(f"非 HTTP 叶仍用泛称结果: {fid}")
    if review.get("confidence") not in {"HIGH", "MEDIUM", "SOURCE_REVIEWED_NOT_LIVE_MIGRATION"}:
        raise ValueError(f"非 HTTP 审查把源码证据误称为真实运行验证: {fid}")
    if feature["kind"] == "provider" and any(claim in review["success_result"] for claim in ("真实调用已通过", "真实服务已验证")):
        raise ValueError(f"供应商目录项不能冒称真实调用已验: {fid}")
    scenario = review.get("scenario")
    if not isinstance(scenario, dict) or not all(isinstance(scenario.get(k), str) and scenario[k].strip() for k in ("given", "when", "then")):
        raise ValueError(f"非 HTTP 叶缺少适用场景: {fid}")
    if not isinstance(scenario.get("assertions"), list) or len(scenario["assertions"]) < 2 or any(not isinstance(x, str) or not x.strip() for x in scenario["assertions"]):
        raise ValueError(f"非 HTTP 叶缺少正反断言: {fid}")
    if not review.get("task_ids"):
        raise ValueError(f"非 HTTP 叶未指定实施 Task: {fid}")
    relations = review.get("old_acceptance_relations")
    if not isinstance(relations, list) or {x.get("id") for x in relations} != set(feature["acceptance_ids"]) or len(relations) != len(feature["acceptance_ids"]):
        raise ValueError(f"非 HTTP 叶旧 A-ID 未逐项裁决: {fid}")
    for relation in relations:
        if relation.get("fit") not in {"DIRECT", "INDIRECT", "GAP"} or not relation.get("reason"):
            raise ValueError(f"非 HTTP 叶旧 A-ID 关系无依据: {fid}")
        if relation["fit"] == "DIRECT" and not relation.get("covers_leaf_result"):
            raise ValueError(f"旧 A-ID 直接验收缺结果证明: {fid} {relation['id']}")
    hashes = review.get("reviewed_source_sha256")
    if not isinstance(hashes, dict) or not hashes:
        raise ValueError(f"非 HTTP 审查没有冻结源码: {fid}")
    for source, expected in hashes.items():
        if source_file_sha256(source) != expected:
            raise ValueError(f"非 HTTP 源码分支签名变化，须重新审查: {fid} {source}")


def review_leaves(review: dict) -> list[dict]:
    leaves = review.get("leaves")
    if not isinstance(leaves, list) or not leaves:
        raise ValueError(f"审查决定没有语义叶: {review.get('entry_id')}")
    for leaf in leaves:
        if not all(isinstance(leaf.get(key), str) and leaf[key].strip() for key in ("semantic_key", "action", "result")):
            raise ValueError(f"语义叶缺少稳定身份或真实结果: {review.get('entry_id')}")
        if not isinstance(leaf.get("assertions"), list) or not leaf["assertions"]:
            raise ValueError(f"语义叶缺少可检查断言: {review.get('entry_id')}")
    if (review.get("decision") == "SPLIT") != (len(leaves) > 1):
        raise ValueError(f"拆叶裁决与叶子数不符: {review.get('entry_id')}")
    return leaves


def handler_signature(source_ref: str) -> str:
    source, start_text = source_ref.rsplit(":", 1)
    source_lines = lines(source)
    start = int(start_text)
    registrations = [i + 1 for i, line in enumerate(source_lines)
                     if re.search(r"\b(?:route|sub)\.(?:get|post|put|patch|delete|on|all)\(", line)]
    end = next((line for line in registrations if line > start), len(source_lines) + 1)
    return hashlib.sha256("\n".join(source_lines[start - 1:end - 1]).encode()).hexdigest()


def validate_semantic_reviews(registries: list[dict]) -> None:
    active = set()
    for raw in registries:
        if raw["kind"] != "route":
            continue
        _, route, method, path = raw["entry_id"].split(":", 3)
        if (route, path) in INTERNAL_ROUTE_OWNER or raw["entry_id"] == CONFIRM_ROUTE:
            continue
        canonical = behavior_key(raw)
        canonical_path = canonical.rsplit(":", 1)[0].split(":", 2)[2]
        effect = "READ" if method in {"GET", "HEAD"} else method
        if (route, canonical_path, effect) not in ROUTE_EFFECTS:
            active.add(canonical)
    frozen = "\n".join(sorted(active))
    if len(active) != BASELINE_PENDING_COUNT or hashlib.sha256(frozen.encode()).hexdigest() != BASELINE_PENDING_IDS_SHA256:
        raise ValueError("普通 route 登记集合或方法变化，需要新增逐源码审查")
    reviews = semantic_reviews()
    if set(reviews) != active:
        raise ValueError(f"逐入口语义审查差集: 缺少={sorted(active - set(reviews))}, 多出={sorted(set(reviews) - active)}")
    semantic_keys = set()
    # 先查每个 handler 自身，再查跨文件 helper/消费者；反例应能指出改变的实际入口。
    for entry_id, review in reviews.items():
        if handler_signature(review["source_refs"][0]) != review.get("reviewed_handler_sha256"):
            raise ValueError(f"handler 分支签名变化，须重新审查: {entry_id}")
    for entry_id, review in reviews.items():
        if review.get("decision") not in {"SPLIT", "SAME_EFFECT_WITH_VARIANTS", "INTERNAL_STEP", "UNREACHABLE"}:
            raise ValueError(f"审查裁决未结案: {entry_id}")
        source_hashes = review.get("reviewed_source_sha256")
        if not isinstance(source_hashes, dict) or not source_hashes:
            raise ValueError(f"审查缺少固定的源码分支签名: {entry_id}")
        for source, expected in source_hashes.items():
            if hashlib.sha256("\n".join(lines(source)).encode()).hexdigest() != expected:
                raise ValueError(f"源码分支签名变化，须重新审查: {entry_id} {source}")
        for leaf in review_leaves(review):
            key = leaf["semantic_key"]
            if key in semantic_keys and review.get("decision") != "SAME_EFFECT_WITH_VARIANTS":
                raise ValueError(f"不同效果误用同一语义身份: {key}")
            semantic_keys.add(key)


def route_semantics(route: str, path: str, method: str, label: str) -> tuple[str, str]:
    if (route, path, method) in ROUTE_EFFECTS:
        return ROUTE_EFFECTS[(route, path, method)]
    review = semantic_reviews().get(f"behavior:{route}:{path}:{method}")
    if not review:
        raise ValueError(f"普通 route 缺少逐源码语义审查: {route} {path} {method}")
    leaf = review_leaves(review)[0]
    return leaf["action"], leaf["result"]


def consolidate(entries: list[dict]) -> dict:
    first = entries[0]
    if first["kind"] != "route":
        return first.copy()
    _, route, _method, _registered_path = first["entry_id"].split(":", 3)
    path = behavior_key(first).rsplit(":", 1)[0].split(":", 2)[2]
    domain = first["parent_domain"]
    methods = sorted({e["entry_id"].split(":", 3)[2] for e in entries})
    effects = {"READ" if m in {"GET", "HEAD"} else m for m in methods}
    if len(effects) != 1:
        raise ValueError(f"不同效果方法被合并: {route} {path} {methods}")
    effect = effects.pop()
    label = route_label(route, path, domain)
    action, result = route_semantics(route, path, effect, label)
    if "HEAD" in methods:
        result = f"GET 返回{label}内容；HEAD 仅返回响应状态和头部，不传响应体"
    merged = first.copy()
    reviewed = (route, path, effect) in ROUTE_EFFECTS or f"behavior:{route}:{path}:{effect}" in semantic_reviews()
    merged.update({"entry_id": behavior_key(first), "kind": "route_behavior", "title": action,
                   "user_action": f"用户{action}",
                   "visible_result": result,
                   "semantic_basis": "源码 handler/消费者逐项复核" if reviewed else "缺少逐源码审查",
                   "semantic_verification": "SOURCE_CHECKED" if reviewed else "PENDING_SOURCE_REVIEW",
                   "current_entrypoints": [e["entry_id"] for e in entries],
                   "source_ref": list(dict.fromkeys(source for e in entries for source in e["source_ref"])),
                   "current_owners": list(dict.fromkeys(owner for e in entries for owner in e["current_owners"])),
                   "current_stores": list(dict.fromkeys(store for e in entries for store in e["current_stores"]))})
    return merged


def leaf_tasks(item: dict) -> tuple[list[str], str]:
    if item["entry_id"] in SPECIAL_TASKS:
        return SPECIAL_TASKS[item["entry_id"]], "文件预览跨资源与桌面宿主验收"
    if item["kind"] == "route_behavior":
        _, route, path, method = item["entry_id"].split(":", 3)
        if route == "mcp" and path == "/session-permissions":
            return ["R04-T03"], "单次工具能力批准属于权限裁决"
        if route == "sessions" and path == "/sessions/workspace-rollback":
            return ["R06-T04", "R08-T07"], "文件回退偏好随会话与前端交互迁移"
        if route in ROUTE_TASK_OVERRIDES:
            return ROUTE_TASK_OVERRIDES[route], f"{route} 生产 route 的实施/验收职责"
    return DOMAINS[item["parent_domain"]][3], "功能域基础场景；逐叶结果见 acceptance_requirement"


def entry(ident: str, kind: str, domain: str, title: str, action: str, result: str, source: str, owner: str, condition: str = "生产可达", classification: str = "保留", store: str | None = None) -> dict:
    return {"entry_id": ident, "kind": kind, "parent_domain": domain, "title": title, "classification": classification,
            "user_action": action, "visible_result": result, "current_entrypoints": [ident], "current_owners": [owner],
            "current_stores": [store or DOMAINS[domain][2]], "source_ref": [source], "reachability": condition}


def mounted_route_files() -> dict[str, str]:
    roots = ["server/composition/open-root.ts", "server/composition/full-root.ts", "server/index.ts"]
    out = {}
    for root in roots:
        for i, line in enumerate(lines(root), 1):
            m = re.search(r'import\s+.*?from\s+"\.?\.?/routes/([\w-]+)\.ts"', line)
            if m:
                out[m.group(1)] = ref(root, i)
    return out


@lru_cache(maxsize=None)
def renderer_consumers(api_name: str) -> list[str]:
    # window.platform 在桌面直接等于 window.hana；也保留局部 hana 别名调用。
    refs = []
    for path in sorted((ROOT / "desktop/src").rglob("*")):
        if path.suffix not in {".ts", ".tsx", ".js", ".jsx"} or "__tests__" in path.parts or path.name.endswith(".test.tsx") or path.name in {"types.ts", "platform.js", "global.d.ts"}:
            continue
        relative = path.relative_to(ROOT).as_posix()
        for i, line in enumerate(lines(relative), 1):
            if re.search(rf"\b{re.escape(api_name)}\s*!?\s*(?:\?\.)?\s*\(", line):
                refs.append(ref(relative, i))
    return refs[:8]


def extract_desktop_host() -> list[dict]:
    preload, main, updater = "desktop/preload.cjs", "desktop/main.cjs", "desktop/auto-updater.cjs"
    pre = lines(preload)
    registrations = {}
    for source, pattern in ((main, r'wrapIpc(?:BestEffort)?Handler\("([^"]+)"'), (updater, r'ipcMain\.handle\("([^"]+)"')):
        for i, line in enumerate(lines(source), 1):
            for channel in re.findall(pattern, line):
                if channel in registrations:
                    raise ValueError(f"重复 IPC handler: {channel}")
                registrations[channel] = ref(source, i)
    bridges = {}
    for i, line in enumerate(pre, 1):
        m = re.search(r'\b(\w+):.*ipcRenderer\.invoke\("([^"]+)"', line)
        if m:
            bridges[m.group(2)] = (m.group(1), ref(preload, i))
    if set(registrations) != set(bridges):
        raise ValueError(f"IPC 注册/桥接不符: handler-only={sorted(set(registrations)-set(bridges))}, bridge-only={sorted(set(bridges)-set(registrations))}")
    if set(bridges) - set(IPC_DOMAINS) or set(IPC_DOMAINS) - set(bridges):
        raise ValueError(f"IPC 无功能归属或旧映射: 新增={sorted(set(bridges)-set(IPC_DOMAINS))}, 失效={sorted(set(IPC_DOMAINS)-set(bridges))}")
    found = []
    for channel, (api, pre_ref) in sorted(bridges.items()):
        domain = IPC_DOMAINS[channel]
        group = IPC_GROUPS.get(channel, channel)
        action, result = IPC_EFFECTS.get(group, (f"使用桌面能力 {channel}", f"桌面返回 {channel} 的实际操作结果"))
        consumers = renderer_consumers(api)
        ident = f"ipc:{channel}"
        item = entry(ident, "ipc", domain, action, f"用户{action}", result, pre_ref, "desktop/main.cjs" if registrations[channel].startswith(main) else updater,
                     "桌面桥已暴露且 handler 已注册；当前静态调用" if consumers else "桌面桥已暴露且 handler 已注册；未发现直接调用（可能经动态平台层）", store=IPC_STORES.get(group, DOMAINS[domain][2]))
        item["source_ref"].extend([registrations[channel], *consumers])
        if registrations[channel].startswith(updater):
            item["source_ref"].extend(["desktop/auto-updater.cjs:802", "desktop/main.cjs:2926"])
        item["current_owners"].extend(["desktop/preload.cjs", *[s.rsplit(":", 1)[0] for s in consumers]])
        found.append(item)
    # send/on 与主进程事件推送也由同一个 preload 桥导出。
    event_refs = {}
    for i, line in enumerate(pre, 1):
        for channel in re.findall(r'ipcRenderer\.(?:send|on)\("([^"]+)"', line):
            event_refs.setdefault(channel, ref(preload, i))
    if set(event_refs) != set(IPC_EVENTS):
        raise ValueError(f"桌面事件无归属或旧映射: 新增={sorted(set(event_refs)-set(IPC_EVENTS))}, 失效={sorted(set(IPC_EVENTS)-set(event_refs))}")
    for channel, pre_ref in sorted(event_refs.items()):
        domain = IPC_EVENTS[channel]
        group = IPC_EVENT_GROUPS.get(channel, channel)
        action, result = IPC_EFFECTS.get(group, (f"接收或发送 {channel} 桌面事件", f"对应窗口更新 {channel} 状态"))
        item = entry(f"ipc-event:{channel}", "ipc_event", domain, action, f"用户触发或接收{action}", result, pre_ref, "desktop/preload.cjs；desktop/main.cjs", store=IPC_STORES.get(group, DOMAINS[domain][2]))
        pre_index = int(pre_ref.rsplit(":", 1)[1]) - 1
        api = next((m.group(1) for line in reversed(pre[:pre_index+1]) if (m := re.search(r'^\s*(\w+):\s*', line))), None)
        if api:
            consumers = renderer_consumers(api)
            item["source_ref"].extend(consumers)
            item["current_owners"].extend(source.rsplit(":", 1)[0] for source in consumers)
        producer = [(source, i) for source in (main, updater) for i, line in enumerate(lines(source), 1) if re.search(rf'\b(?:wrapIpcOn|\.send|sendToRenderer)\("{re.escape(channel)}"', line)]
        item["source_ref"].extend(ref(source, i) for source, i in producer[:5])
        found.append(item)
    return found


def extract_slash_commands() -> list[dict]:
    source = "core/slash-commands/bridge-commands.ts"
    body = "\n".join(lines(source)).split("export const bridgeCommands = [", 1)[1].split("const LOOP_SUBCOMMANDS", 1)[0]
    names = re.findall(r'^\s+name: "([\w-]+)"', body, re.M)
    if set(names) != set(SLASH_DOMAINS) or len(names) != len(set(names)):
        raise ValueError(f"核心 slash 注册无归属或重复: 新增={sorted(set(names)-set(SLASH_DOMAINS))}, 失效={sorted(set(SLASH_DOMAINS)-set(names))}")
    index_source = "core/slash-commands/index.ts"
    if not any("for (const def of bridgeCommands) registry.registerCommand(def)" in line for line in lines(index_source)):
        raise ValueError("核心 slash 注册循环已变化")
    found = []
    for name in names:
        i = next(i for i, line in enumerate(lines(source), 1) if re.search(rf'^\s+name: "{re.escape(name)}"', line))
        domain = SLASH_DOMAINS[name]
        action, result = SLASH_EFFECTS[name]
        item = entry(f"slash-command:{name}", "slash_command", domain, f"/{name} {action}", f"用户在 Bridge 或聊天输入 /{name}", result, ref(source, i), "core/slash-commands；Bridge/Chat", store=DOMAINS[domain][2])
        item["source_ref"].extend(["core/slash-commands/index.ts:25", "core/engine.ts:863", "lib/bridge/bridge-manager.ts:1139", "server/routes/chat.ts:2538"])
        found.append(item)
        next_lines = lines(source)[i:i+4]
        for j, line in enumerate(next_lines, i+1):
            if "aliases:" in line:
                for alias in re.findall(r'"([\w-]+)"', line):
                    found.append(entry(f"slash-alias:{alias}", "slash_alias", domain, f"/{alias} 是 /{name} 别名", f"用户输入 /{alias}", result, ref(source, j), "core/slash-commands", store=DOMAINS[domain][2]))
    return found


UI_PROJECTIONS = [
    ("service-connectivity", "D20", "本地服务连接与诊断", "用户启动桌面或网页客户端", "客户端连接本地服务；健康探针和前端诊断上报支持错误提示", ["desktop/src/modules/platform.js:4", "desktop/src/modules/platform.js:50", "server/index.ts:1027", "server/index.ts:1059"], "本地服务运行态；诊断日志"),
    ("mood-live", "D06", "实时 MOOD 展示", "用户阅读实时生成的心情块", "流事件组成可折叠 MOOD/PULSE/REFLECT 块，正文不混入协议标签", ["server/routes/chat.ts:1438", "desktop/src/react/hooks/use-stream-buffer.ts:774", "desktop/src/react/components/chat/MoodBlock.tsx:20"], "session JSONL；会话流状态"),
    ("mood-history", "D06", "历史 MOOD 重开", "用户重新打开带心情块的旧消息", "历史正文被解析为独立心情块，其他文字保持顺序", ["desktop/src/react/utils/message-parser.ts:49", "desktop/src/react/utils/assistant-block-builder.ts:107", "desktop/src/react/components/chat/AssistantMessage.tsx:1432"], "session JSONL"),
    ("five-language", "D23", "五语言界面切换", "用户在界面设置中选择简体/繁体中文、日文、韩文或英文", "配置保存后界面立即加载所选语言，重启后仍使用该语言", ["desktop/src/react/settings/tabs/InterfaceTab.tsx:592", "desktop/src/react/settings/tabs/InterfaceTab.tsx:600", "desktop/src/react/settings/tabs/InterfaceTab.tsx:601"], "用户 locale 配置；desktop/src/locales 五语言资源"),
    ("native-file-drag", "D23", "原生文件拖放", "用户把书桌或聊天文件拖出窗口", "系统拖放携带实际文件路径，可放入 Finder 或其他窗口", ["desktop/src/react/components/desk/DeskTree.tsx:561", "desktop/src/react/components/chat/AssistantMessage.tsx:576", "desktop/preload.cjs:193"], "工作区文件；会话文件"),
    ("system-notification", "D23", "系统通知展示", "用户收到 Agent 消息或任务提醒", "系统通知显示标题正文，并按 Agent 选用头像", ["desktop/src/react/services/ws-message-handler.ts:1020", "desktop/main.cjs:6249"], "通知运行态；Agent 头像文件"),
    ("file-preview", "D09", "文件预览与刷新", "用户打开会话或书桌文件预览", "读取快照并显示内容，文件变化后刷新预览", ["desktop/src/react/utils/file-preview.ts:134", "desktop/src/react/utils/preview-file-content.ts:100", "desktop/main.cjs:6134"], "工作区文件；预览快照"),
    ("browser-visible", "D14", "可见浏览器窗口", "用户打开 Agent 使用的浏览器", "独立可见窗口可导航和切换标签，页面状态返回主界面", ["desktop/preload.cjs:169", "desktop/main.cjs:5479", "desktop/src/react/browser-viewer/BrowserViewerApp.tsx:64"], "浏览器 profile/cookie；会话标签"),
    ("quick-chat", "D01", "快捷聊天窗口", "用户通过全局快捷键打开快捷聊天并发送消息", "悬浮窗口接入同一本地服务，可切回完整会话", ["desktop/preload.cjs:82", "desktop/src/react/quick-chat/QuickChatApp.tsx:320", "desktop/src/react/quick-chat/QuickChatApp.tsx:677"], "session JSONL；快捷窗口状态"),
    ("observability-export-save", "D22", "模型观测另存文件", "用户在用量观测界面选择导出并指定保存位置", "大文件分块保存到用户所选位置，失败或取消中止", ["desktop/src/react/settings/tabs/observability/observability-export-save.ts:38", "desktop/main.cjs:5804", "desktop/main.cjs:5863"], "model-observability/observability.sqlite；导出文件"),
]


def extract_ui_projections() -> list[dict]:
    found = []
    for key, domain, title, action, result, sources, store in UI_PROJECTIONS:
        for source in sources:
            path, number = source.rsplit(":", 1)
            if not (ROOT / path).is_file() or int(number) > len(lines(path)):
                raise ValueError(f"界面行为证据失效: {key} {source}")
        item = entry(f"ui-behavior:{key}", "ui_behavior", domain, title, action, result, sources[0], sources[0].rsplit(":", 1)[0], store=store)
        item["source_ref"] = sources
        item["current_owners"] = list(dict.fromkeys(source.rsplit(":", 1)[0] for source in sources))
        found.append(item)
    return found


def extract() -> list[dict]:
    found = []
    mounted = mounted_route_files()
    missing = set(mounted) - set(ROUTE_DOMAINS)
    if missing:
        raise ValueError(f"新增已挂载 route 未归属: {sorted(missing)}")
    for route, mount_ref in sorted(mounted.items()):
        route_path = f"server/routes/{route}.ts"
        domain = ROUTE_DOMAINS[route]
        reachability = "main-full 完整产品入口" if "full-root.ts" in mount_ref else ("server/index 直接挂载，产品开闭归属待单独确认" if route == "mobile-workbench" else "生产可达")
        ident = f"route-mount:{route}"
        found.append(entry(ident, "route_mount", domain, f"{route} 路由挂载", f"用户打开或调用 {route} 相关能力", "入口进入对应服务", mount_ref, route_path, reachability, store=ROUTE_STORES[route]))
        for i, line in enumerate(lines(route_path), 1):
            # 精确到注册调用，限定路径以 / 开头，排除普通 Map.get 等成员调用。
            for match in re.finditer(r'\b\w+\.(get|post|put|patch|delete|on|all)\s*\(\s*[\'\"](/[^\'\"]*)[\'\"]', line):
                method, path = match.group(1).upper(), match.group(2)
                item_domain = route_domain(route, path)
                ident = f"route:{route}:{method}:{path}"
                found.append(entry(ident, "route", item_domain, f"{DOMAINS[item_domain][0]} · {path}",
                                   f"用户在{DOMAINS[item_domain][0]}中{ACTION.get(method, '调用')} {path} 对应操作",
                                   RESULT.get(method, "收到操作结果"), ref(route_path, i), route_path,
                                   f"{reachability}；需有效配置/权限" if route in {"bridge", "media", "mcp", "git-environment"} else reachability, store=ROUTE_STORES[route]))
            for match in re.finditer(r'\b\w+\.on\s*\(\s*[\'\"]([A-Z]+)[\'\"]\s*,\s*[\'\"](/[^\'\"]*)[\'\"]', line):
                method, path = match.group(1), match.group(2)
                item_domain = route_domain(route, path)
                ident = f"route:{route}:{method}:{path}"
                found.append(entry(ident, "route", item_domain, f"{DOMAINS[item_domain][0]} · {path}",
                                   f"用户读取 {path} 对应资源", "收到资源响应", ref(route_path, i), route_path, store=ROUTE_STORES[route]))
    # index 直接注册的服务控制端点也属于生产入口。
    index_path = "server/index.ts"
    for i, line in enumerate(lines(index_path), 1):
        for match in re.finditer(r'\bapp\.(get|post|put|patch|delete)\s*\(\s*[\'\"](/[^\'\"]*)[\'\"]', line):
            method, path = match.group(1).upper(), match.group(2)
            domain = "D04" if "permission" in path or "plan-mode" in path else "D20"
            ident = f"route:server-index:{method}:{path}"
            found.append(entry(ident, "route", domain, f"服务控制 · {path}", f"用户{ACTION[method]} {path} 对应操作",
                               RESULT[method], ref(index_path, i), index_path))
    # 动态 helper 注册的静态客户端端点无法由文字路径正则看到。
    mobile = "server/routes/mobile-static.ts"
    for base in ("mobile", "desktop"):
        ident = f"route:mobile-static:GET:/{base}[/*]"
        found.append(entry(ident, "route", "D20", f"/{base} 静态客户端", f"用户打开 /{base}",
                           "看到网页客户端、安装说明或明确错误", ref(mobile, 57 if base == "mobile" else 58), mobile,
                           "取决于构建产物，dist/guide/error 三态"))
    for alias, line_no in (("/mcp", 381), ("/plugins/mcp", 385)):
        ident = f"route-alias:mcp:{alias}"
        found.append(entry(ident, "route_alias", "D03", f"MCP 路由别名 {alias}", f"用户或旧客户端调用 {alias}",
                           "进入同一 MCP 管理端点", ref("server/routes/mcp.ts", line_no), "server/routes/mcp.ts"))
    chat = "server/routes/chat.ts"
    seen_ws = set()
    for i, line in enumerate(lines(chat), 1):
        for kind in re.findall(r'msg\.type === "([a-z_]+)"', line):
            if kind not in WS_DOMAINS or kind in seen_ws:
                continue
            seen_ws.add(kind)
            ident = f"ws-in:{kind}"
            domain = WS_DOMAINS[kind]
            found.append(entry(ident, "ws_in", domain, f"WS {kind}", f"用户在会话中执行 {kind} 操作",
                               "收到对应流事件、状态或错误", ref(chat, i), "server/routes/chat.ts；hub/index.ts"))
    # 主界面页面、设置导航与真实组件表，二者交集才能算可操作 tab。
    nav = "desktop/src/react/settings/SettingsNav.tsx"
    component = "desktop/src/react/settings/SettingsContent.tsx"
    component_ids = set(re.findall(r"^\s*(\w+): \w+Tab,", "\n".join(lines(component)), re.M))
    for i, line in enumerate(lines(nav), 1):
        m = re.search(r"\{ id: '([\w-]+)', key: 'settings\.tabs\.", line)
        if not m:
            continue
        tab = m.group(1)
        if tab not in component_ids or tab not in UI_DOMAINS:
            raise ValueError(f"设置页无组件或归属: {tab}")
        domain = UI_DOMAINS[tab]
        ident = f"ui:settings:{tab}"
        found.append(entry(ident, "ui", domain, f"设置页 · {tab}", f"用户打开 {tab} 设置页", "看到对应设置并可操作", ref(nav, i), component))
    page = "desktop/src/react/components/app/AppPages.tsx"
    for i, line in enumerate(lines(page), 1):
        for tab in re.findall(r"currentTab === '([\w-]+)' && <\w+Page", line):
            if tab not in UI_DOMAINS:
                raise ValueError(f"主页面无归属: {tab}")
            ident = f"ui:page:{tab}"
            found.append(entry(ident, "ui", UI_DOMAINS[tab], f"主页面 · {tab}", f"用户切换到 {tab} 页面", "看到页面内容", ref(page, i), page))
        for panel, domain in (("Activity", "D18"), ("Automation", "D18"), ("Bridge", "D17"), ("Skills", "D21")):
            if f"<{panel}Panel" in line and "import" not in line:
                ident = f"ui:panel:{panel.lower()}"
                found.append(entry(ident, "ui", domain, f"{panel} 面板", f"用户打开 {panel} 面板", "看到对应任务或配置", ref(page, i), page))
    # 工具类别为生产注册的静态闭集；LEGACY_INTERNAL 不计现役入口。
    cats = "shared/tool-categories.ts"
    tool_implementations = {}
    for tool_path in sorted((ROOT / "lib/tools").glob("*.ts")) + sorted((ROOT / "lib/memory").glob("*.ts")):
        relative = tool_path.relative_to(ROOT).as_posix()
        for line_no, tool_line in enumerate(lines(relative), 1):
            for implemented_name in re.findall(r'\bname:\s*"([a-z][\w]*)"', tool_line):
                tool_implementations.setdefault(implemented_name, ref(relative, line_no))
    cat = None
    for i, line in enumerate(lines(cats), 1):
        m = re.search(r"export const (CORE|STANDARD|GLOBAL|OPTIONAL|LEGACY_INTERNAL)_TOOL_NAMES", line)
        if m:
            cat = m.group(1)
        if cat and "];" in line:
            cat = None
        if not cat or cat == "LEGACY_INTERNAL":
            continue
        for name in re.findall(r'"([a-z][\w]*)"', line):
            if name not in TOOL_DOMAINS:
                raise ValueError(f"工具无归属: {name}")
            domain = TOOL_DOMAINS[name]
            plugin_toggle = name in {"beautify", "office"}
            ident = f"tool-toggle:{name}" if plugin_toggle else f"tool:{name}"
            condition = "可选，默认关闭" if name == "workflow" else ("按 Agent/全局开关启用" if cat in {"OPTIONAL", "GLOBAL"} else ("可按数据/会话开关启用" if name in {"search_memory", "pin_memory", "unpin_memory", "subagent"} else "生产注册"))
            item = entry(ident, "plugin_toggle" if plugin_toggle else "tool", domain,
                         f"{name} 内置插件开关" if plugin_toggle else f"{name} 工具",
                         f"用户启用 {name} 内置插件" if plugin_toggle else f"用户允许 Agent 调用 {name}",
                         "插件具体工具进入目录" if plugin_toggle else "工具结果或产物进入会话",
                         ref(cats, i), "core/agent.ts；core/engine.ts", condition)
            if name in tool_implementations:
                item["source_ref"].append(tool_implementations[name])
                item["current_owners"].append(tool_implementations[name].split(":", 1)[0])
            elif name in {"read", "write", "edit", "exec_command", "write_stdin", "grep", "find", "ls", "materialize"}:
                item["source_ref"].append("lib/sandbox/index.ts:86")
                item["current_owners"].append("lib/sandbox/index.ts")
            elif name in {"ast_edit", "ast_grep", "lsp", "run_code", "security_scan"}:
                item["source_ref"].append("lib/sandbox/index.ts:200")
                item["current_owners"].append("lib/sandbox/index.ts")
            elif name == "todo_write":
                item["source_ref"].append("lib/tools/todo.ts:26")
                item["current_owners"].append("lib/tools/todo.ts")
            elif name == "run_tools":
                item["source_ref"].append("core/engine.ts:4641")
                item["current_owners"].append("core/engine.ts")
            elif plugin_toggle:
                item["source_ref"].append(f"plugins/{name}/lib/availability.ts:3")
                item["current_owners"].append(f"plugins/{name}/lib/availability.ts")
            found.append(item)
    bridge_tools = "core/tool-catalog-bridge.ts"
    bridge_text = "\n".join(lines(bridge_tools))
    match = re.search(r'export const BRIDGE_TOOL_NAMES = \[([^\]]+)\]', bridge_text)
    if not match:
        raise ValueError("按需工具桥注册表未找到")
    for name in re.findall(r'"([a-z][\w]+)"', match.group(1)):
        line_no = next(i for i, line in enumerate(lines(bridge_tools), 1) if "BRIDGE_TOOL_NAMES" in line)
        ident = f"tool:{name}"
        found.append(entry(ident, "tool", "D03", f"{name} 工具目录桥", f"用户允许 Agent 调用 {name}",
                           "查找、查看或执行按需工具", ref(bridge_tools, line_no), "core/engine.ts"))
    providers = "core/provider-registry.ts"
    provider_lines = lines(providers)
    in_builtins = False
    for i, line in enumerate(provider_lines, 1):
        if line.startswith("const BUILTIN_PLUGINS = ["):
            in_builtins = True
            continue
        if in_builtins and "];" in line:
            break
        if in_builtins:
            m = re.match(r"\s*(\w+Plugin),\s*(?://.*)?$", line)
            if m:
                name = m.group(1).removesuffix("Plugin")
                ident = f"provider:{name}"
                item = entry(ident, "provider", "D10", f"模型供应商 {name}", f"用户配置或选择 {name} 模型",
                             "模型出现在配置与可用模型列表", ref(providers, i), "core/provider-registry.ts", "供应商配置有效时可调用")
                plugin_file = ROOT / "lib/providers" / f"{re.sub(r'(?<!^)(?=[A-Z])', '-', name).lower()}.ts"
                if plugin_file.is_file():
                    item["source_ref"].append(ref(plugin_file.relative_to(ROOT).as_posix(), 1))
                    item["current_owners"].append(plugin_file.relative_to(ROOT).as_posix())
                found.append(item)
    if not any(x["entry_id"] == "provider:ollama" for x in found):
        raise ValueError("Ollama 接入未在供应商注册表中找到")
    experiment = "lib/experiments/registry.ts"
    for i, line in enumerate(lines(experiment), 1):
        if 'PROACTIVE_SUBAGENT_EXPERIMENT_ID = "subagent.proactive_delegation"' in line:
            found.append(entry("experiment:subagent.proactive_delegation", "experiment", "D05", "现役子代理主动委派实验开关",
                               "用户在实验设置中开启或关闭主动委派", "新会话按开关改变主动委派提示", ref(experiment, i), "core/agent.ts；ExperimentsTab.tsx", "beta，默认 false"))
            break
    # 调度真实启动边界；各平台适配器从 PLATFORM_SPECS 注册表提取。
    sched = "hub/scheduler.ts"
    for key, pattern, domain, title in [
        ("heartbeat", "this.startHeartbeat();", "D18", "定期心跳"),
        ("cron", "this._startStudioCron();", "D18", "定时任务"),
        ("fresh-compact", "this._freshCompactScheduler.start();", "D18", "每日新鲜压缩维护"),
    ]:
        hits = [i for i, line in enumerate(lines(sched), 1) if pattern in line]
        if not hits:
            raise ValueError(f"调度入口未找到: {key}")
        ident = f"scheduler:{key}"
        found.append(entry(ident, "scheduler", domain, title, f"用户配置或等待 {title}", "后台任务触发并留下结果", ref(sched, hits[0]), sched))
    bridge = "lib/bridge/bridge-manager.ts"
    text = "\n".join(lines(bridge))
    if "const ADAPTER_REGISTRY" not in text:
        raise ValueError("Bridge adapter 注册表未找到")
    spec = text.split("const ADAPTER_REGISTRY", 1)[-1].split("\n};", 1)[0]
    for platform in re.findall(r"^\s+(telegram|feishu|dingtalk|qq|wechat):\s*\{", spec, re.M):
        line_no = next(i for i, line in enumerate(lines(bridge), 1) if re.match(rf"\s+{platform}:\s*\{{", line))
        ident = f"bridge:{platform}"
        found.append(entry(ident, "bridge_adapter", "D17", f"{platform} 平台收发", f"用户在 {platform} 发送消息或附件", "收到 Agent 回复或附件", ref(bridge, line_no), bridge, "配置有效且已启用"))
    # 内置插件按随包 manifest 登记；外装插件和 MCP 的具体实例不在此闭集。
    for manifest in sorted((ROOT / "plugins").glob("*/manifest.json")):
        plugin = manifest.parent.name
        source = manifest.relative_to(ROOT).as_posix()
        ident = f"builtin-plugin:{plugin}"
        found.append(entry(ident, "builtin_plugin", "D16", f"内置插件 {plugin}", f"用户启用 {plugin} 并调用其能力", "插件返回结果或文件", ref(source, 1), source, "随包分发，按启用状态可达"))
        for tool_file in sorted((manifest.parent / "tools").glob("*.ts")):
            tool_source = tool_file.relative_to(ROOT).as_posix()
            ident = f"builtin-plugin-tool:{plugin}:{tool_file.stem}"
            found.append(entry(ident, "builtin_plugin_tool", "D16", f"{plugin} · {tool_file.stem}", f"用户调用 {plugin} 的 {tool_file.stem}", "看到执行结果或产物", ref(tool_source, 1), tool_source, "插件启用时可达"))
        if plugin == "jimeng-cli":
            adapter_source = "plugins/jimeng-cli/index.ts"
            for capability, factory in (("image", "createJimengImageAdapter"), ("video", "createJimengVideoAdapter")):
                line_no = next(i for i, line in enumerate(lines(adapter_source), 1) if f"{factory}(" in line)
                ident = f"builtin-plugin-adapter:jimeng-cli:{capability}"
                found.append(entry(ident, "builtin_plugin_adapter", "D16", f"即梦 CLI {capability} 生成",
                                   f"用户通过即梦 CLI 生成{ '图片' if capability == 'image' else '视频' }", "收到生成文件或明确失败",
                                   ref(adapter_source, line_no), adapter_source, "随包适配器已注册，执行还需本机 dreamina 命令与授权"))
    # CLI 子命令和构建/更新入口并非 HTTP route，明确登记为叶子。
    cli = "cli/entry.ts"
    for i, line in enumerate(lines(cli), 1):
        if line.startswith("function shouldAutoStartServer"):
            break
        m = re.search(r'args\.command === "(serve|status|sessions|continue|chat|bundle|data)"', line)
        if m:
            command = m.group(1)
            ident = f"cli:{command}"
            found.append(entry(ident, "cli", "D20", f"CLI {command}", f"用户执行 CLI {command}",
                               "终端显示结果或进入会话", ref(cli, i), cli))
    for i, line in enumerate(lines(cli), 1):
        m = re.search(r'args\.subcommand === "(pull|diagnose|checkpoints)"', line)
        if m:
            command = m.group(1)
            ident = f"cli:subcommand:{command}"
            found.append(entry(ident, "cli", "D20", f"CLI 子命令 {command}", f"用户执行 {command} 子命令",
                               "终端显示本地处理结果", ref(cli, i), cli))
    for ident, label, marker in (("cli:subcommand:bundle-status", "查看 bundle 状态", "runBundleStatus"),
                                 ("cli:subcommand:data-restore", "恢复数据检查点", "runDataRestore"),
                                 ("cli:help", "显示 CLI 帮助", 'args.command === "help"')):
        line_no = next(i for i, line in enumerate(lines(cli), 1) if marker in line and "import" not in line)
        found.append(entry(ident, "cli", "D20", label, f"用户在 CLI 中{label}", "终端显示结果或错误",
                           ref(cli, line_no), cli))
    for source, pattern, label in [
        ("desktop/src/shared/github-release-check.cjs", r'github', "检查 GitHub Releases 更新"),
        ("desktop/main.cjs", r'train-update-', "更新列车控制"),
    ]:
        hits = [i for i, line in enumerate(lines(source), 1) if re.search(pattern, line, re.I)]
        if not hits:
            raise ValueError(f"更新入口未找到: {label}")
        ident = f"update:{Path(source).name}"
        found.append(entry(ident, "update", "D24", label, f"用户{label}", "看到检查、下载或安装状态", ref(source, hits[0]), source))
    found.extend(extract_desktop_host())
    found.extend(extract_slash_commands())
    found.extend(extract_ui_projections())
    # 兼容地址必须调用同一 handler，不能仅凭相似路径合并。
    by_route_id = {x["entry_id"]: x for x in found if x["kind"] == "route"}
    for item in list(by_route_id.values()):
        _, route, method, path = item["entry_id"].split(":", 3)
        canonical = behavior_key(item).rsplit(":", 1)[0].split(":", 2)[2]
        if canonical == path:
            continue
        peer = by_route_id.get(f"route:{route}:{method}:{canonical}")
        if not peer:
            raise ValueError(f"兼容地址没有现行同伴: {item['entry_id']}")
        source_line = lines(item["source_ref"][0].rsplit(":", 1)[0])[int(item["source_ref"][0].rsplit(":", 1)[1])-1]
        peer_line = lines(peer["source_ref"][0].rsplit(":", 1)[0])[int(peer["source_ref"][0].rsplit(":", 1)[1])-1]
        if source_line.replace(path, canonical).strip() != peer_line.strip():
            raise ValueError(f"兼容地址不再指向同一 handler: {item['entry_id']}")
    # 稳定且唯一。重复的注册身份意味着提取或代码本身存在歧义。
    ids = [item["entry_id"] for item in found]
    duplicates = sorted({x for x in ids if ids.count(x) > 1})
    if duplicates:
        raise ValueError(f"重复生产入口: {duplicates}")
    return sorted(found, key=lambda x: x["entry_id"])



# 第二轮审阅的 72 组多方法路由，冻结方法集合；增加、删除方法须逐组重新裁决。
MULTI_METHOD_CONTRACT = {
    'behavior:access:/access/account/password': 'DELETE,PUT',
    'behavior:agents:/agents': 'GET,POST',
    'behavior:agents:/agents/:id/agents-md': 'GET,PUT',
    'behavior:agents:/agents/:id/avatar': 'DELETE,GET,POST',
    'behavior:agents:/agents/:id/config': 'GET,PUT',
    'behavior:agents:/agents/:id/experience': 'GET,PUT',
    'behavior:agents:/agents/:id/identity': 'GET,PUT',
    'behavior:agents:/agents/:id/pinned': 'GET,PUT',
    'behavior:agents:/agents/:id/public-agents-md': 'GET,PUT',
    'behavior:agents:/agents/:id/tenets': 'GET,POST',
    'behavior:auth:/auth/oauth/:provider/custom-models': 'GET,POST',
    'behavior:auth:/auth/oauth/start': 'GET,POST',
    'behavior:avatar:/avatar/:role': 'DELETE,GET,POST',
    'behavior:cards:/cards/:cardId': 'GET,PUT',
    'behavior:channels:/channels': 'GET,POST',
    'behavior:channels:/channels/:name': 'DELETE,GET',
    'behavior:channels:/conversations/:id/agent-phone-settings': 'GET,POST',
    'behavior:channels:/conversations/:id/agent-phone-tool-mode': 'GET,POST',
    'behavior:config:/config': 'GET,PUT',
    'behavior:config:/config/default-workspace': 'GET,POST',
    'behavior:config:/config/workspaces/recent': 'DELETE,POST',
    'behavior:config:/memories': 'DELETE,GET',
    'behavior:config:/memories/compiled': 'DELETE,GET',
    'behavior:config:/user-profile': 'GET,PUT',
    'behavior:conversation-map:/conversation-map/layout': 'GET,PUT',
    'behavior:desk:/desk/cron': 'GET,POST',
    'behavior:desk:/desk/files': 'GET,POST',
    'behavior:desk:/desk/jian': 'GET,POST',
    'behavior:experiments:/experiments/memory/cache-snapshot-reflection/observation': 'DELETE,GET',
    'behavior:html-preview:/preview/html/:id': 'GET,HEAD',
    'behavior:html-preview:/preview/html/:id/assets/:token/*': 'GET,HEAD',
    'behavior:input-drafts:/input-drafts': 'GET,PUT',
    'behavior:knowledge:/knowledge/notebooks': 'GET,POST',
    'behavior:knowledge:/knowledge/notebooks/:id': 'DELETE,GET,PATCH',
    'behavior:knowledge:/knowledge/notebooks/:id/sources': 'GET,POST',
    'behavior:mcp:/connectors/:id': 'DELETE,PUT',
    'behavior:mcp:/connectors/:id/oauth/start': 'GET,POST',
    'behavior:media:/media/image/providers/:providerId/models/:modelId': 'DELETE,PUT',
    'behavior:media:/media/speech/config': 'GET,PUT',
    'behavior:media:/media/speech/providers/:providerId/models/:modelId': 'DELETE,PUT',
    'behavior:media:/media/video/providers/:providerId/models/:modelId': 'DELETE,PUT',
    'behavior:mobile-workbench:/workbench/content': 'GET,HEAD',
    'behavior:model-observability:/model-observability/blobs/:blobId': 'GET,HEAD',
    'behavior:model-observability:/model-observability/settings': 'GET,PUT',
    'behavior:preferences:/preferences/appearance': 'GET,PUT',
    'behavior:preferences:/preferences/autolearn': 'GET,PUT',
    'behavior:preferences:/preferences/browser': 'GET,PUT',
    'behavior:preferences:/preferences/computer-use': 'GET,PUT',
    'behavior:preferences:/preferences/computer-use/approvals': 'DELETE,POST',
    'behavior:preferences:/preferences/goal': 'GET,PUT',
    'behavior:preferences:/preferences/keybindings': 'GET,PUT',
    'behavior:preferences:/preferences/models': 'GET,PUT',
    'behavior:preferences:/preferences/notifications': 'GET,PUT',
    'behavior:preferences:/preferences/quick-chat': 'GET,PUT',
    'behavior:preferences:/preferences/session-permission-default': 'GET,PUT',
    'behavior:preferences:/preferences/sidebar-ui': 'GET,PUT',
    'behavior:preferences:/preferences/workspace-ui-state': 'GET,PUT',
    'behavior:providers:/providers/:name/models/:modelId': 'DELETE,PUT',
    'behavior:resources:/resources/:resourceId/content': 'GET,HEAD',
    'behavior:server-index:/api/plan-mode': 'GET,POST',
    'behavior:server-index:/api/session-permission-mode': 'GET,POST',
    'behavior:server-index:/api/session-thinking-level': 'GET,POST',
    'behavior:session-projects:/session-projects/folders/:id': 'DELETE,PATCH',
    'behavior:session-projects:/session-projects/projects/:id': 'DELETE,PATCH',
    'behavior:sessions:/sessions/authorized-folders': 'GET,PATCH',
    'behavior:sessions:/sessions/memory': 'GET,PATCH',
    'behavior:sessions:/sessions/workspace-rollback': 'GET,PUT',
    'behavior:skills:/skills/bundles': 'GET,POST',
    'behavior:skills:/skills/bundles/:id': 'DELETE,PUT',
    'behavior:skills:/skills/external-paths': 'GET,PUT',
    'behavior:speech-recognition:/speech-recognition/providers/:providerId/models/:modelId': 'DELETE,PUT',
    'behavior:studio-workspaces:/studio/workspaces': 'GET,POST',
}

def build() -> tuple[dict, dict, dict]:
    task_catalog = json.loads((TASKBOOK / "task-catalog.json").read_text())
    acceptance_catalog = json.loads((TASKBOOK / "acceptance-catalog.json").read_text())
    tasks = {t["id"]: t for t in task_catalog["tasks"]}
    acceptances = {a["id"] for a in acceptance_catalog["scenarios"]}
    registries = extract()
    if not REQUIRED_INTERNAL.issubset(INTERNAL_ROUTE_OWNER):
        raise ValueError("内部端点失去能力归属")
    validate_semantic_reviews(registries)
    active_effect_keys = set()
    for raw in registries:
        if raw["kind"] != "route":
            continue
        _, route, method, path = raw["entry_id"].split(":", 3)
        canonical = behavior_key(raw).rsplit(":", 1)[0].split(":", 2)[2]
        active_effect_keys.add((route, canonical, "READ" if method in {"GET", "HEAD"} else method))
    if set(ROUTE_EFFECTS) - active_effect_keys:
        raise ValueError(f"逐项语义声明已无生产入口: {sorted(set(ROUTE_EFFECTS) - active_effect_keys)}")
    by_id = {item["entry_id"]: item for item in registries}
    required_ui = {"ui-behavior:mood-live": "D06", "ui-behavior:mood-history": "D06", "ui-behavior:five-language": "D23", "ui-behavior:native-file-drag": "D23", "ui-behavior:system-notification": "D23", "ui-behavior:file-preview": "D09"}
    for ident, domain in required_ui.items():
        if ident not in by_id or by_id[ident]["parent_domain"] != domain:
            raise ValueError(f"无独立 URL 的受保护行为漏账: {ident}")
    additional_entrypoints = {}
    for item in registries:
        ident = item["entry_id"]
        if item["kind"] == "route_mount":
            route = ident.split(":", 1)[1]
            candidates = sorted(e for e in by_id if e.startswith(f"route:{route}:"))
            if not candidates:
                raise ValueError(f"挂载路由无叶子行为: {route}")
            owner_key = "body-effect:confirm:confirmed" if route == "confirm" else behavior_key(by_id[candidates[0]])
            additional_entrypoints.setdefault(owner_key, []).append(item)
        elif item["kind"] == "route_alias":
            additional_entrypoints.setdefault("behavior:mcp:/state:READ", []).append(item)
        elif item["kind"] == "route" and (ident.split(":", 3)[1], ident.split(":", 3)[3]) in INTERNAL_ROUTE_OWNER:
            _, route, _, path = ident.split(":", 3)
            additional_entrypoints.setdefault(INTERNAL_ROUTE_OWNER[(route, path)], []).append(item)
        elif ident == CONFIRM_ROUTE:
            continue
        elif item["kind"] == "slash_alias":
            alias = ident.split(":", 1)[1]
            name = item["title"].split(" 是 /", 1)[-1].split(" ", 1)[0]
            if name not in SLASH_DOMAINS:
                raise ValueError(f"slash 别名失去主命令: {alias}")
            additional_entrypoints.setdefault(f"slash-command:{name}", []).append(item)
        elif item["kind"] == "ui_behavior" and ident.split(":", 1)[1] in UI_PROJECTION_OWNER:
            additional_entrypoints.setdefault(UI_PROJECTION_OWNER[ident.split(":", 1)[1]], []).append(item)
        elif item["kind"] == "builtin_plugin":
            plugin = ident.split(":", 1)[1]
            candidates = sorted(e for e in by_id if e.startswith(f"builtin-plugin-tool:{plugin}:"))
            if plugin == "jimeng-cli":
                candidates = ["builtin-plugin-adapter:jimeng-cli:image"]
            if not candidates:
                raise ValueError(f"内置插件没有可达叶子: {plugin}")
            additional_entrypoints.setdefault(candidates[0], []).append(item)
        elif ident == "cli:bundle":
            additional_entrypoints.setdefault("cli:subcommand:bundle-status", []).append(item)
        elif ident == "cli:data":
            additional_entrypoints.setdefault("cli:subcommand:data-restore", []).append(item)
    grouped = {}
    for item in registries:
        if item["kind"] in {"route_mount", "route_alias", "builtin_plugin", "slash_alias"} or item["entry_id"] in {"cli:bundle", "cli:data", CONFIRM_ROUTE} or item["entry_id"].startswith("ui-behavior:") and item["entry_id"].split(":", 1)[1] in UI_PROJECTION_OWNER:
            continue
        if item["kind"] == "route":
            _, route, _, path = item["entry_id"].split(":", 3)
            if (route, path) in INTERNAL_ROUTE_OWNER:
                continue
        if item["kind"] in {"ipc", "ipc_event"}:
            channel = item["entry_id"].split(":", 1)[1]
            key = IPC_EVENT_GROUPS.get(channel, channel) if item["kind"] == "ipc_event" else FILE_EDIT_SPLITS.get(channel, IPC_GROUPS.get(channel, channel))
            grouped.setdefault(f"desktop-behavior:{key}", []).append(item)
        else:
            grouped.setdefault(behavior_key(item), []).append(item)
    features = []
    for group_key, group_items in grouped.items():
        item = consolidate(group_items)
        if group_key.startswith("desktop-behavior:"):
            item = group_items[0].copy()
            item["entry_id"] = group_key
            item["kind"] = "desktop_behavior"
            item["current_entrypoints"] = [x["entry_id"] for x in group_items]
            item["source_ref"] = list(dict.fromkeys(source for x in group_items for source in x["source_ref"]))
            item["current_owners"] = list(dict.fromkeys(owner for x in group_items for owner in x["current_owners"]))
            item["current_stores"] = list(dict.fromkeys(store for x in group_items for store in x["current_stores"]))
            item["reachability"] = "桌面桥与 handler 已注册；已找到 renderer 直接调用" if any(any(s.startswith("desktop/src/") for s in x["source_ref"]) for x in group_items) else "桌面桥与 handler 已注册；未找到 renderer 直接调用，迁移时按能力边界复核"
            action, result = IPC_EFFECTS.get(group_key.split(":", 1)[1], (item["title"], item["visible_result"]))
            item.update(title=action, user_action=f"用户{action}", visible_result=result)
        domain = item["parent_domain"]
        task_ids, task_basis = leaf_tasks(item)
        acceptance_ids = [a for task in task_ids for a in tasks[task]["acceptance_ids"]]
        stages = sorted({t[:3] for t in task_ids})
        feature = {"feature_id": make_id(domain, item["kind"].upper(), item["entry_id"]), **item,
                   "target_owner": DOMAINS[domain][1], "stage_id": stages[0], "stage_ids": stages,
                   "task_ids": task_ids, "acceptance_ids": acceptance_ids,
                   "task_mapping_basis": task_basis,
                   "acceptance_requirement": f"{item['user_action']} → {item['visible_result']}",
                   "platform_scope": ["macOS", "Windows", "Linux", "Web/Mobile"] if domain in {"D01", "D02", "D08", "D09", "D20"} else ["macOS", "Windows", "Linux"],
                   "status": "INVENTORIED_NOT_IMPLEMENTED", "evidence": [item["source_ref"][0]]}
        if item["entry_id"] == "behavior:model-observability:/model-observability/health:READ":
            feature["source_ref"].extend([
                "desktop/src/react/settings/tabs/observability/model-observability-actions.ts:204",
                "desktop/src/react/settings/tabs/observability/ModelObservabilitySection.tsx:99",
                "desktop/src/react/settings/tabs/observability/ModelObservabilitySection.tsx:142",
            ])
            feature["acceptance_scope"] = "打开用量页；分别准备可用、未初始化、无权限与网络失败状态"
            feature["acceptance_when"] = "页面自动读取 /model-observability/health"
            feature["acceptance_then"] = "可用状态与录制状态如实显示；无权限和网络失败显示 alert，不冒充空数据"
            feature["acceptance_assertions"] = [
                "成功时核对健康状态进入用量页，且记录关闭时历史查询仍可用",
                "未初始化状态按实际状态显示，不冒充有记录",
                "无权限与网络失败显示可见错误提示，不渲染成空用量",
            ]
        if item["entry_id"] == "ui-behavior:file-preview":
            feature["target_owner"] = "Tauri DesktopHost＋Rust Resource＋React"
            feature["supporting_entrypoints"] = ["ipc:read-file-snapshot", "ipc:watch-file", "ipc-event:file-changed", "ipc:read-docx-html", "ipc:read-xlsx-html"]
            feature["acceptance_ids"] = [a for a in feature["acceptance_ids"] if not a.startswith("R09-")] + PREVIEW_ACCEPTANCE
            feature["source_ref"].append("desktop/src/react/utils/file-preview.ts:110")
            feature["separate_from"] = {"feature_key": "desktop-behavior:file-edit", "evidence": "desktop/src/react/components/PreviewEditor.tsx:635"}
        for supplemental in additional_entrypoints.get(item["entry_id"], []):
            feature["current_entrypoints"].append(supplemental["entry_id"])
            feature["source_ref"].extend(supplemental["source_ref"])
            feature["evidence"].extend(supplemental["source_ref"])
        review = semantic_reviews().get(item["entry_id"]) if item["kind"] == "route_behavior" else None
        if review:
            for leaf in review_leaves(review):
                semantic_entry = f"semantic-effect:{leaf['semantic_key']}"
                reviewed_feature = {**feature,
                                    "feature_id": make_id(domain, "SEMANTIC_EFFECT", semantic_entry),
                                    "entry_id": semantic_entry,
                                    "semantic_key": leaf["semantic_key"],
                                    "title": leaf["action"], "user_action": f"用户{leaf['action']}",
                                    "visible_result": leaf["result"],
                                    "semantic_decision": review["decision"],
                                    "semantic_review_entry_id": review["entry_id"],
                                    "route_registration_key": item["entry_id"],
                                    "consumer_scope": review.get("consumer_scope", "BUILTIN_OR_INDIRECT_CALL_REVIEWED"),
                                    "review_confidence": review.get("confidence", "SOURCE_AND_STATIC_CONSUMER_SEARCH_REVIEWED"),
                                    "review_limitations": review.get("unresolved", []),
                                    "acceptance_scope": review.get("acceptance_scope", "以具备相应权限的 HTTP 客户端直接请求；不宣称存在现役内置按钮" if review.get("consumer_scope") == "EXTERNAL_HTTP_NO_BUILTIN_CALL_FOUND" else "按已查调用者和 HTTP 返回核对；尚未执行新实现验收"),
                                    "semantic_basis": "逐 handler、helper 及 HTTP 结果审查；内置消费者查找范围见 consumer_scope" if review.get("consumer_scope") == "EXTERNAL_HTTP_NO_BUILTIN_CALL_FOUND" else "逐 handler、helper、消费者及结果审查；见语义审查报告",
                                    "semantic_verification": "SOURCE_CHECKED_WITH_LIMITATION" if review.get("unresolved") or review.get("consumer_scope") == "EXTERNAL_HTTP_NO_BUILTIN_CALL_FOUND" else "SOURCE_CHECKED",
                                    "acceptance_assertions": leaf["assertions"],
                                    "acceptance_requirement": "；".join(leaf["assertions"]),
                                    "current_entrypoints": list(feature["current_entrypoints"]),
                                    "source_ref": list(feature["source_ref"]),
                                    "evidence": list(feature["evidence"])}
                if semantic_entry == "semantic-effect:agent.config.global_update":
                    reviewed_feature["current_stores"] = ["preferences.json（全局字段 setter）"]
                elif semantic_entry == "semantic-effect:agent.config.update":
                    reviewed_feature["current_stores"] = ["agents/{id}/config.yaml（splitByScope 后仍有助手字段时）"]
                elif semantic_entry in {"semantic-effect:provider.agent.save", "semantic-effect:provider.agent.remove",
                                        "semantic-effect:provider.inline_credential.save", "semantic-effect:provider.global.save",
                                        "semantic-effect:provider.global.remove"}:
                    reviewed_feature["storage_contract"] = leaf["storage_contract"]
                    reviewed_feature["current_stores"] = leaf["current_stores"]
                    reviewed_feature["acceptance_scope"] = leaf["acceptance_scope"]
                    reviewed_feature["acceptance_when"] = leaf["acceptance_when"]
                    reviewed_feature["acceptance_then"] = leaf["acceptance_then"]
                suggested_tasks = leaf.get("suggested_task_ids")
                if suggested_tasks and set(suggested_tasks) <= tasks.keys():
                    reviewed_feature["task_ids"] = suggested_tasks
                    reviewed_feature["stage_ids"] = sorted({x[:3] for x in suggested_tasks})
                    reviewed_feature["stage_id"] = reviewed_feature["stage_ids"][0]
                suggested_acceptance = leaf.get("suggested_acceptance_ids")
                if suggested_acceptance and set(suggested_acceptance) <= acceptances:
                    reviewed_feature["acceptance_ids"] = suggested_acceptance
                features.append(reviewed_feature)
        else:
            features.append(feature)
    # confirm 的同一 POST 按 body.action 走批准/拒绝两种相反结果；原始入口为两叶共享。
    if CONFIRM_ROUTE not in by_id or not any('["confirmed", "rejected"].includes(action)' in line for line in lines("server/routes/confirm.ts")):
        raise ValueError("确认路由的 body.action 契约变化")
    for value, (action, result, sources) in CONFIRM_EFFECTS.items():
        task_ids = ["R04-T03"]
        features.append({"feature_id": make_id("D04", "BODY_EFFECT", f"confirm:{value}"),
                         "entry_id": f"body-effect:confirm:{value}", "kind": "body_effect",
                         "parent_domain": "D04", "title": action, "classification": "保留",
                         "user_action": f"用户在确认卡选择{action}", "visible_result": result,
                         "current_entrypoints": [CONFIRM_ROUTE, *[x["entry_id"] for x in additional_entrypoints.get(f"body-effect:confirm:{value}", [])]], "shared_entrypoint_reason": "同一 POST 的 body.action 分为相反的用户决定",
                         "current_owners": ["server/routes/confirm.ts", "desktop/src/react/components/chat/SettingsConfirmCard.tsx"],
                         "current_stores": ["ConfirmStore 待处理记录；会话流事件"],
                         "source_ref": sources, "reachability": "确认卡出现且请求范围获授权",
                         "target_owner": DOMAINS["D04"][1], "stage_id": "R04", "stage_ids": ["R04"],
                         "task_ids": task_ids, "acceptance_ids": [a for task in task_ids for a in tasks[task]["acceptance_ids"]],
                         "platform_scope": ["macOS", "Windows", "Linux", "Web/Mobile"],
                         "status": "INVENTORIED_NOT_IMPLEMENTED", "evidence": sources})
    # 逐叶审查不仅适用于 HTTP。支持入口保留来源/权限断言，同时明确指向
    # 它承载的语义叶；页面和供应商变体也要有真实检查点。
    nonhttp = nonhttp_reviews()
    nonhttp_features = {f["feature_id"]: f for f in features if f["kind"] != "route_behavior"}
    if set(nonhttp) != set(nonhttp_features):
        raise ValueError(f"非 HTTP 逐叶审查差集: 缺少={sorted(set(nonhttp_features)-set(nonhttp))}, 多出={sorted(set(nonhttp)-set(nonhttp_features))}")
    all_feature_ids = {f["feature_id"] for f in features}
    for fid, feature in nonhttp_features.items():
        review = nonhttp[fid]
        reviewed_tasks = review.get("task_ids", [])
        if not reviewed_tasks or set(reviewed_tasks) - tasks.keys():
            raise ValueError(f"非 HTTP 实施 Task 无效: {fid}")
        feature["task_ids"] = reviewed_tasks
        feature["stage_ids"] = sorted({task[:3] for task in reviewed_tasks})
        feature["stage_id"] = feature["stage_ids"][0]
        feature["acceptance_ids"] = list(dict.fromkeys(a for task in reviewed_tasks for a in tasks[task]["acceptance_ids"]))
        validate_nonhttp_review(feature, review, all_feature_ids)
        feature["identity_decision"] = review["decision"]
        feature["semantic_identity"] = review.get("semantic_identity", feature["title"])
        feature["user_action"] = review["user_action"]
        feature["visible_result"] = review["success_result"]
        feature["refusal_or_boundary"] = review["refusal_or_boundary"]
        feature["branch_variants"] = review.get("branch_variants", [])
        feature["variant_group"] = review.get("variant_group")
        feature["supported_feature_ids"] = review.get("supported_feature_ids", [])
        feature["call_chain"] = review.get("call_chain", [])
        feature["review_confidence"] = review.get("confidence")
        feature["review_limitations"] = review.get("limitations", [])
        feature["semantic_basis"] = "逐叶追踪生产调用、最终结果与拒绝边界；见非 HTTP 审查报告"
        feature["semantic_verification"] = "SOURCE_CHECKED_WITH_LIMITATION" if feature["review_limitations"] else "SOURCE_CHECKED"
        feature["acceptance_requirement"] = "；".join(review["scenario"]["assertions"])
        feature["acceptance_assertions"] = review["scenario"]["assertions"]
        feature["acceptance_scope"] = review["scenario"]["given"]
        feature["acceptance_when"] = review["scenario"]["when"]
        feature["acceptance_then"] = review["scenario"]["then"]
        feature["existing_acceptance_fit"] = {"assessment": "逐项见 existing_acceptance_relations；现有 A-ID 不替代新子场景"}
        feature["existing_acceptance_relations"] = [
            {"acceptance_id": row["id"], "relation": row["fit"], "covers_leaf_result": row["covers_leaf_result"], "reason": row["reason"]}
            for row in review["old_acceptance_relations"]
        ]
        feature["task_mapping_basis"] = "非 HTTP 逐叶源码审查后的实施归属"
        feature["evidence"] = list(dict.fromkeys(feature["evidence"] + review.get("source_refs", [])))
    # 原任务包中的许多 A-ID 是域级横向约束。全部保留叶均附可直接检查
    # 动作、正反结果及支持关系的子场景，不把阶段编号当作逐叶完成证据。
    for feature in features:
        if feature["kind"] == "route_behavior" and not feature.get("acceptance_assertions"):
            feature["acceptance_assertions"] = [
                f"执行“{feature['title']}”后核对：{feature['visible_result']}",
                "失败、冲突或拒绝时核对返回的状态与实际副作用一致",
            ]
        review = semantic_reviews().get(feature.get("semantic_review_entry_id", "")) if feature["kind"] == "route_behavior" else None
        if feature["kind"] == "route_behavior":
            feature["existing_acceptance_fit"] = review.get("acceptance_fit", {"assessment": "补充逐叶断言；现有 ID 保留为阶段约束"}) if review else {"assessment": "补充逐叶断言；现有 ID 保留为阶段约束"}
        feature["supplemental_acceptance_id"] = "R00-T02-LA-" + hashlib.sha256(feature["entry_id"].encode()).hexdigest()[:12].upper()
        # 旧 A-ID 多为阶段横向条件，不能冒充该叶的成功/失败检查。
        # R00-T07 负责把逐叶子场景纳入正式账本，实施 Task 在自己的阶段验收前执行。
        if feature["kind"] == "route_behavior":
            fit = feature["existing_acceptance_fit"]
            partial_ids = {row["id"] for row in fit.get("scenario_checks", []) if row.get("fit") == "PARTIAL"}
            feature["existing_acceptance_relations"] = [
                {"acceptance_id": acceptance_id,
                 "relation": "INDIRECT" if acceptance_id in partial_ids or fit.get("verdict") == "NOT_ESTABLISHED_FROM_CURRENT_SCENARIOS" else "GAP",
                 "covers_leaf_result": False,
                 "reason": "现有场景只覆盖该叶的部分或横向边界，未检查完整动作及结果" if acceptance_id in partial_ids or fit.get("verdict") == "NOT_ESTABLISHED_FROM_CURRENT_SCENARIOS" else "未找到该 A-ID 对本叶成功、拒绝及副作用的直接检查；保留阶段关联，由逐叶场景补足"}
                for acceptance_id in feature["acceptance_ids"]
            ]
        feature["acceptance_formalization_task_id"] = "R00-T07"
        feature["acceptance_execution_task_ids"] = feature["task_ids"]
        feature["acceptance_due"] = "R00-T07 建立正式账本时登记；最迟在所列实施 Task 的阶段验收前完成并执行"
        if not feature["task_ids"] or not feature["acceptance_assertions"] or any(not assertion.strip() for assertion in feature["acceptance_assertions"]):
            raise ValueError(f"逐叶验收缺负责人或可执行断言: {feature['feature_id']}")
    scenario_by_feature = {f["feature_id"]: f["supplemental_acceptance_id"] for f in features}
    for feature in nonhttp_features.values():
        feature["supported_scenario_ids"] = [scenario_by_feature[fid] for fid in feature["supported_feature_ids"]]
    matrix_path = OUT / UI_ACTION_MATRIX_FILE
    if not matrix_path.exists():
        raise ValueError(f"缺少 26 页逐动作支持矩阵: {UI_ACTION_MATRIX_FILE}")
    ui_matrix = json.loads(matrix_path.read_text(encoding="utf-8"))
    validate_ui_review_fields(ui_matrix, nonhttp)
    validate_ui_matrix(ui_matrix,
                       [f for f in features if f["kind"] == "ui"],
                       {f["feature_id"]: f for f in features}, ROOT, lines)
    for page in ui_matrix["pages"]:
        feature = next(f for f in features if f["entry_id"] == page["entry_id"])
        feature["ui_action_checks"] = page["actions"]
        feature["acceptance_scope"] = f"打开 {page['entry_id']}，准备成功、空态或未初始化、失败或无权限样本"
        feature["acceptance_when"] = "逐一执行 UI 动作矩阵记录的自动加载与可见控件"
        feature["acceptance_then"] = "逐动作核对实际状态与页面投影；失败不伪装成空数据"
        feature["acceptance_assertions"] = [
            f"{action['trigger']}：成功核对 {action['success']}；空态/未初始化核对 {action['empty_or_uninitialized']}；错误/无权限核对 {action['failure_or_forbidden']}"
            for action in page["actions"]
        ]
        feature["acceptance_requirement"] = "；".join(feature["acceptance_assertions"])
    # 双向关联：每个抽取入口恰好由一个叶子拥有；每个叶子至少一个真实入口。
    ownership_pairs = [(ep, f["feature_id"]) for f in features for ep in f["current_entrypoints"]]
    expected_semantic_keys = sorted(leaf["semantic_key"] for review in semantic_reviews().values() for leaf in review_leaves(review))
    actual_semantic_keys = sorted(f["semantic_key"] for f in features if f.get("semantic_key"))
    if expected_semantic_keys != actual_semantic_keys:
        raise ValueError("逐入口审查决定未完整落到语义叶")
    owners = dict(ownership_pairs)
    owner_sets = {ep: sorted({fid for key, fid in ownership_pairs if key == ep}) for ep, _ in ownership_pairs}
    reviewed_split_entries = {ep for review in semantic_reviews().values() if review["decision"] == "SPLIT"
                              for ep in next((f["current_entrypoints"] for f in features
                                              if f.get("semantic_review_entry_id") == review["entry_id"]), [])}
    duplicate_entry_owners = sorted({ep for ep, ids in owner_sets.items() if len(ids) > 1
                                     and ep != CONFIRM_ROUTE and ep not in reviewed_split_entries})
    extracted = {e["entry_id"] for e in registries}
    delta = {"unowned_entries": sorted(extracted - owners.keys()),
             "inventory_entries_not_registered": sorted(owners.keys() - extracted),
             "duplicate_feature_ids": sorted({f["feature_id"] for f in features if sum(x["feature_id"] == f["feature_id"] for x in features) > 1}),
             "duplicate_entry_owners": duplicate_entry_owners}
    support_pairs = [(ep, f["feature_id"]) for f in features for ep in f.get("supporting_entrypoints", [])]
    if set(ep for ep, _ in support_pairs) - extracted:
        raise ValueError("共享底层入口不存在")
    preview = next((f for f in features if f["entry_id"] == "ui-behavior:file-preview"), None)
    if (not preview or preview["parent_domain"] != "D09" or set(SPECIAL_TASKS["ui-behavior:file-preview"]) != set(preview["task_ids"])
            or set(PREVIEW_ACCEPTANCE) != {"R09-A09", "R09-A10"}
            or not {"R09-T05"}.issubset(preview["task_ids"]) or not {"R09-A09", "R09-A10"}.issubset(preview["acceptance_ids"])):
        raise ValueError("文件预览独立叶子或 R09 验收映射缺失")
    if owners.get("ui-behavior:file-preview") != preview["feature_id"] or owners.get("ipc:read-file-snapshot") == preview["feature_id"]:
        raise ValueError("文件预览错误归并到编辑")
    if len(owner_sets.get(CONFIRM_ROUTE, [])) != 2 or set(CONFIRM_EFFECTS) != {"confirmed", "rejected"}:
        raise ValueError("确认请求的批准/拒绝叶子缺失")
    risk_terms = ("push", "pull", "fetch", "import", "permission", "approve", "reject", "revoke", "rollback", "restore", "logout", "cleanup", "delete", "clear", "discard")
    side_effect_reviews = []
    for feature in features:
        if feature["kind"] != "route_behavior":
            continue
        _, route, path, method = feature.get("route_registration_key", feature["entry_id"]).split(":", 3)
        if method != "DELETE" and not any(term in path for term in risk_terms):
            continue
        side_effect_reviews.append({"feature_id": feature["feature_id"], "route": route, "method": method, "path": path,
                                    "action": feature["user_action"], "visible_result": feature["visible_result"],
                                    "source_ref": feature["source_ref"], "semantic_basis": feature.get("semantic_basis", "")})
    if any(x["semantic_basis"] != "源码 handler/消费者逐项复核" for x in side_effect_reviews):
        raise ValueError(f"高副作用路由仍靠模板推断: {[x['feature_id'] for x in side_effect_reviews if x['semantic_basis'] != '源码 handler/消费者逐项复核']}")
    pending_semantics = [{"feature_id": f["feature_id"], "entry_id": f["entry_id"], "source_ref": f["source_ref"],
                          "pending": "逐 handler/消费者确认用户动作、数据或外部副作用、可见结果"}
                         for f in features if f["kind"] == "route_behavior" and f["semantic_verification"] == "PENDING_SOURCE_REVIEW"]
    legacy_id_map = []
    for item in registries:
        if item["kind"] != "route":
            continue
        _, route, _, path = item["entry_id"].split(":", 3)
        old_behavior = f"behavior:{route}:{path}"
        old_id = make_id(item["parent_domain"], "ROUTE_BEHAVIOR", old_behavior)
        r2_id = make_id(item["parent_domain"], "ROUTE_BEHAVIOR", behavior_key(item))
        for new_id in owner_sets[item["entry_id"]]:
            if old_id != new_id:
                legacy_id_map.append({"old_feature_id": old_id, "new_feature_id": new_id, "entry_id": item["entry_id"], "reason": "按真实结果拆分；兼容地址或内部协议保持归属"})
            if r2_id != new_id:
                legacy_id_map.append({"old_feature_id": r2_id, "new_feature_id": new_id, "entry_id": item["entry_id"], "reason": "R3 逐 handler 审查把方法级旧叶映射到稳定语义效果"})
    old_file_edit_id = make_id("D09", "DESKTOP_BEHAVIOR", "desktop-behavior:file-edit")
    health_feature = next((f for f in features if f["entry_id"] == "behavior:model-observability:/model-observability/health:READ"), None)
    if health_feature is None:
        raise ValueError("观测 health 必需独立叶缺失")
    legacy_id_map.append({
        "old_feature_id": make_id("D22", "SEMANTIC_EFFECT", "semantic-effect:model-observability.model_observability_settings.read"),
        "new_feature_id": health_feature["feature_id"],
        "entry_id": "route:model-observability:GET:/model-observability/health",
        "reason": "旧观测设置读取叶曾误收 health：服务状态与持久化设置返回不同；拆出页面 bootstrap 状态和错误结果",
    })
    legacy_id_map.append({"old_feature_id": old_file_edit_id, "new_feature_id": preview["feature_id"], "entry_id": "ui-behavior:file-preview", "reason": "旧文件编辑叶子曾错误包含只读文件预览；拆出 R09 桌面预览"})
    for split_key in sorted(set(FILE_EDIT_SPLITS.values())):
        split_entry = f"desktop-behavior:{split_key}"
        split_feature = next((f for f in features if f["entry_id"] == split_entry), None)
        if not split_feature:
            raise ValueError(f"文件编辑拆分叶失踪: {split_entry}")
        legacy_id_map.append({"old_feature_id": old_file_edit_id, "new_feature_id": split_feature["feature_id"],
                              "entry_id": split_entry, "reason": "旧文件编辑组合叶混合不同文件/编辑效果；按真实 IPC 副作用拆分"})
    for value in CONFIRM_EFFECTS:
        legacy_id_map.append({"old_feature_id": make_id("D04", "ROUTE_BEHAVIOR", "behavior:confirm:/confirm/:confirmId"), "new_feature_id": make_id("D04", "BODY_EFFECT", f"confirm:{value}"), "entry_id": CONFIRM_ROUTE, "reason": "同一 POST 按 body.action 拆为批准与拒绝"})
    legacy_id_map = sorted({(x["old_feature_id"], x["new_feature_id"]): x for x in legacy_id_map}.values(), key=lambda x: x["old_feature_id"])
    # 保留 R2 指出的全部同 URL 多方法集合，逐组记录拆分或同读裁决与原始源码。
    method_groups = {}
    feature_by_id = {feature["feature_id"]: feature for feature in features}
    for item in registries:
        if item["kind"] != "route":
            continue
        _, route, method, path = item["entry_id"].split(":", 3)
        old_key = INTERNAL_ROUTE_OWNER.get((route, path), behavior_key(item))
        old_key = old_key.rsplit(":", 1)[0]
        method_groups.setdefault(old_key, []).append(item)
    multi_method_decisions = []
    for key, group in sorted(method_groups.items()):
        methods = sorted({item["entry_id"].split(":", 3)[2] for item in group})
        if len(methods) < 2 or not key.startswith("behavior:"):
            continue
        is_internal = any((item["entry_id"].split(":", 3)[1], item["entry_id"].split(":", 3)[3]) in INTERNAL_ROUTE_OWNER for item in group)
        user_effects = {fid for item in group for fid in owner_sets[item["entry_id"]]
                        if (item["entry_id"].split(":", 3)[1], item["entry_id"].split(":", 3)[3]) not in INTERNAL_ROUTE_OWNER}
        internal_only_extra = is_internal and len(user_effects) == 1
        effect_ids = sorted({fid for item in group for fid in owner_sets[item["entry_id"]]})
        method_effects = []
        for method in methods:
            members = [x for x in group if x["entry_id"].split(":", 3)[2] == method]
            representative = next((x for x in members if (x["entry_id"].split(":", 3)[1], x["entry_id"].split(":", 3)[3]) not in INTERNAL_ROUTE_OWNER), members[0])
            _, route, _, path = representative["entry_id"].split(":", 3)
            canonical_path = behavior_key(representative).rsplit(":", 1)[0].split(":", 2)[2]
            effect = "READ" if method in {"GET", "HEAD"} else method
            internal_method = (route, path) in INTERNAL_ROUTE_OWNER
            if internal_method:
                action, result = "完成登录内部回调或轮询", "更新所属登录状态，不独立成为用户功能"
            else:
                label = route_label(route, path, representative["parent_domain"])
                action, result = route_semantics(route, canonical_path, effect, label)
            reviewed = internal_method or (route, canonical_path, effect) in ROUTE_EFFECTS or f"behavior:{route}:{canonical_path}:{effect}" in semantic_reviews()
            method_effects.append({"method": method, "action": action, "visible_result": result,
                                   "semantic_basis": "源码 handler/消费者逐项复核" if reviewed else "缺少逐源码审查",
                                   "entry_ids": [x["entry_id"] for x in members],
                                   "source_ref": [x["source_ref"][0] for x in members],
                                   "feature_ids": sorted({fid for x in members for fid in owner_sets[x["entry_id"]]}),
                                   "leaf_effects": [{"feature_id": fid, "action": feature_by_id[fid]["user_action"], "visible_result": feature_by_id[fid]["visible_result"]}
                                                    for fid in sorted({fid for x in members for fid in owner_sets[x["entry_id"]]})]})
        if set(methods) <= {"GET", "HEAD"}:
            rationale = "GET 与 HEAD 读取同一资源；HEAD 只省略响应体，合并为同一读取行为"
        elif internal_only_extra:
            rationale = "OAuth 回调/轮询是 POST 发起登录的内部协议步骤，不独立计用户功能"
        else:
            rationale = "；".join(f"{x['method']}：{x['action']}" for x in method_effects) + "；效果不同，分别验收"
        multi_method_decisions.append({"old_behavior_key": key, "methods": methods,
                                       "decision": "SHARED_READ" if set(methods) <= {"GET", "HEAD"} else ("INTERNAL_STEP" if internal_only_extra else "SPLIT"),
                                       "reason": rationale, "method_effects": method_effects, "entry_ids": [x["entry_id"] for x in group],
                                       "source_ref": [x["source_ref"][0] for x in group],
                                       "feature_ids": effect_ids})
    actual_contract = {x["old_behavior_key"]: ",".join(x["methods"]) for x in multi_method_decisions}
    if actual_contract != MULTI_METHOD_CONTRACT:
        raise ValueError(f"多方法裁决集合变化，需要复审: 新增或改变={sorted(actual_contract.items() - MULTI_METHOD_CONTRACT.items())}")
    if not any('["push"]' in line for line in lines("server/git/git-command.ts")) or not any('["push", "-u"' in line for line in lines("server/git/git-command.ts")):
        raise ValueError("Git 推送实际副作用证据变化")
    if "git push" not in ROUTE_EFFECTS[("git-environment", "/git/push", "POST")][1]:
        raise ValueError("Git 推送副作用文字失真")
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    nonproduction = [
        {"feature_id": "F-D05-X-FORCED-SUBAGENT-ROUTING", "parent_domain": "D05", "title": "专用子代理目录与强制分发改造", "classification": "已撤回", "user_action": "无现役用户入口", "visible_result": "不恢复强制路由", "current_entrypoints": [], "current_owners": ["无生产注册"], "current_stores": [], "source_ref": ["docs/refactor-2026/P00/FEATURE_MATRIX.md:43"], "target_owner": "无", "stage_id": [], "task_ids": [], "acceptance_ids": ["R00-A04"], "platform_scope": [], "status": "EXCLUDED", "evidence": ["lib/experiments/registry.ts:15"]},
        {"feature_id": "F-D08-X-INDEPENDENT-RESEARCH", "parent_domain": "D08", "title": "独立知识研究运行引擎", "classification": "已撤回", "user_action": "无现役启动入口", "visible_result": "不新建研究运行", "current_entrypoints": [], "current_owners": ["无生产注册"], "current_stores": [], "source_ref": ["lib/knowledge/knowledge-store.ts:1901"], "target_owner": "无", "stage_id": [], "task_ids": [], "acceptance_ids": ["R00-A04"], "platform_scope": [], "status": "EXCLUDED", "evidence": ["server/routes/chat.ts:1955"]},
        {"feature_id": "F-D10-X-LOCAL-MODEL-MANAGEMENT", "parent_domain": "D10", "title": "本地模型下载/安装/生命周期管理子系统", "classification": "已撤回", "user_action": "无现役用户入口", "visible_result": "不增加本地模型管理", "current_entrypoints": [], "current_owners": ["无生产注册"], "current_stores": [], "source_ref": ["docs/refactor-2026/P00/FEATURE_MATRIX.md:45"], "target_owner": "无", "stage_id": [], "task_ids": [], "acceptance_ids": ["R00-A04"], "platform_scope": [], "status": "EXCLUDED", "evidence": ["core/provider-registry.ts:480"]},
        {"feature_id": "F-D08-C-RESEARCH-LEDGER", "parent_domain": "D08", "title": "旧研究表与历史事件兼容", "classification": "兼容残留", "user_action": "用户打开旧会话或旧知识来源", "visible_result": "既有记录可读取/展示，来源删除仍受引用保护", "current_entrypoints": ["旧会话历史投影"], "current_owners": ["lib/knowledge/knowledge-store.ts", "server/routes/chat.ts", "desktop/src/react/components/chat/MessageActivity.tsx"], "current_stores": ["knowledge/knowledge.db schema v18/v19", "session JSONL"], "source_ref": ["lib/knowledge/knowledge-store.ts:1901", "lib/knowledge/knowledge-store.ts:3692", "server/routes/chat.ts:1955"], "target_owner": "Rust Knowledge 兼容读取", "stage_id": ["R06", "R08"], "task_ids": ["R06-T07", "R08-T04"], "acceptance_ids": ["R06-A13", "R06-A14", "R08-A07", "R08-A08"], "platform_scope": ["macOS", "Windows", "Linux"], "status": "COMPATIBILITY_ONLY", "evidence": ["desktop/src/react/utils/tool-label.ts:31"]},
        {"feature_id": "F-D03-E-USER-MCP", "parent_domain": "D03", "title": "用户配置的外部 MCP 连接器", "classification": "外装扩展", "user_action": "用户自行添加连接器", "visible_result": "经统一目录/权限边界调用其工具", "current_entrypoints": ["route:mcp:POST:/connectors", "tool:mcp_call"], "current_owners": ["core/mcp/manager.ts"], "current_stores": ["plugin-data/mcp"], "source_ref": ["server/routes/mcp.ts:245", "core/engine.ts:594"], "target_owner": "Rust MCP 协议/网关", "stage_id": ["R04"], "task_ids": ["R04-T07"], "acceptance_ids": ["R04-A13", "R04-A14"], "platform_scope": ["macOS", "Windows", "Linux"], "status": "EXTERNAL_INSTANCE_NOT_BUILTIN", "evidence": ["core/engine.ts:594"]},
        {"feature_id": "F-D16-E-USER-PLUGIN-LEGACY", "parent_domain": "D16", "title": "旧用户插件目录", "classification": "兼容残留", "user_action": "旧版本中用户曾安装插件", "visible_result": "当前版本提示旧目录不再加载", "current_entrypoints": [], "current_owners": ["core/engine.ts"], "current_stores": ["~/.lingxi/plugins"], "source_ref": ["core/engine.ts:3755", "core/engine.ts:3761"], "target_owner": "R07 扩展生命周期决策", "stage_id": ["R07"], "task_ids": ["R07-T11"], "acceptance_ids": ["R07-A21", "R07-A22"], "platform_scope": ["macOS", "Windows", "Linux"], "status": "COMPATIBILITY_ONLY", "evidence": ["core/engine.ts:3779"]},
        {"feature_id": "F-D16-E-DYNAMIC-SLASH-PROTOCOL", "parent_domain": "D16", "title": "插件动态斜杠命令注册边界", "classification": "外装扩展", "user_action": "插件在 commands 目录提供 handler 后登记命令", "visible_result": "受 full-access 与保留名检查约束；具体外装命令名随安装变化", "current_entrypoints": ["core/plugin-manager.ts:_loadCommands"], "current_owners": ["core/plugin-manager.ts", "core/slash-command-registry.ts"], "current_stores": ["插件目录及 manifest"], "source_ref": ["core/plugin-manager.ts:1006", "core/plugin-manager.ts:1029"], "target_owner": "Rust 插件命令注册与权限边界", "stage_id": ["R04", "R07"], "task_ids": ["R04-T07", "R07-T11"], "acceptance_ids": ["R04-A13", "R04-A14", "R07-A21", "R07-A22"], "platform_scope": ["macOS", "Windows", "Linux"], "status": "EXTERNAL_INSTANCE_NOT_BUILTIN", "evidence": ["core/plugin-manager.ts:1029"]},
    ]
    all_features = features + nonproduction
    if len({f["feature_id"] for f in all_features}) != len(all_features):
        raise ValueError("全部 F-ID 有重复")
    for feature in all_features:
        if feature["parent_domain"] not in DOMAINS:
            raise ValueError(f"未知功能域: {feature['feature_id']}")
        if set(feature["task_ids"]) - tasks.keys() or set(feature["acceptance_ids"]) - acceptances:
            raise ValueError(f"任务或验收映射失效: {feature['feature_id']}")
        for source in feature["source_ref"]:
            match = re.fullmatch(r"(.+):(\d+)", source)
            if not match or not (ROOT / match.group(1)).is_file() or int(match.group(2)) > len(lines(match.group(1))):
                raise ValueError(f"源码定位失效: {feature['feature_id']} {source}")
    if any(f["classification"] != "保留" for f in features):
        raise ValueError("撤回/外装/兼容项进入开发功能清单")
    if set(LEGACY_MATRIX_DOMAINS) != {f"F{i:02}" for i in range(1, 21)} or set(d for ds in LEGACY_MATRIX_DOMAINS.values() for d in ds) != set(DOMAINS):
        raise ValueError("旧 F01-F20 与 D01-D24 对账缺项")
    inventory = {"schema_version": 1, "task_id": "R00-T02", "tested_sha": head, "baseline_sha": "16aeb380d58d68ff1a38bb46f5cc5d18f985f084",
                 "classification_values": ["保留", "外装扩展", "已撤回", "兼容残留", "DECISION_REQUIRED"],
                 "domain_count": len(DOMAINS), "feature_count": len(features), "classified_nonproduction_count": len(nonproduction),
                 "pending_route_semantics_count": len(pending_semantics),
                 "domains": [{"id": k, "name": v[0], "target_owner": v[1], "current_store_summary": v[2]} for k, v in DOMAINS.items()],
                 "features": features, "classified_nonproduction": nonproduction, "legacy_feature_id_map": legacy_id_map,
                 "legacy_matrix_crosswalk": [{"legacy_id": old, "domain_ids": domains} for old, domains in LEGACY_MATRIX_DOMAINS.items()],
                 "decisions_required": [{"id": "R00-T02-DR01", "classification": "DECISION_REQUIRED", "subject": "mobile-workbench 的最终开放/闭集交付归属", "observed": "server/index.ts 直接挂载该 route；server/composition/open-root.ts 注释将它标为 evidence-needed。现役功能本身保留。", "source_ref": ["server/index.ts:1022", "server/composition/open-root.ts:7"], "owner_stage": "R01/R07"}],
                 "scope_notes": ["生产 route factory 挂载、其内 HTTP 注册、主/设置 UI、静态工具目录、调度、Bridge、内置插件、CLI 与更新入口逐项登记。", "条件入口仍属于保留功能；外装 MCP/插件的具体实例不算内置功能。", "研究相关旧事件/表属于兼容残留，见 EXCLUSIONS.md。"]}
    mapping = {"schema_version": 2, "task_id": "R00-T02", "tested_sha": head,
               "features": [{"feature_id": f["feature_id"], "classification": f["classification"], "domain_id": f["parent_domain"], "stage_ids": f.get("stage_ids", f["stage_id"]), "task_ids": f["task_ids"], "acceptance_ids": f["acceptance_ids"],
                             **({"identity_decision": f["identity_decision"], "supported_feature_ids": f["supported_feature_ids"], "supported_scenario_ids": f["supported_scenario_ids"], "variant_group": f.get("variant_group")} if f.get("identity_decision") else {}),
                             **({"supplemental_acceptance_id": f["supplemental_acceptance_id"], "acceptance_assertions": f["acceptance_assertions"], "existing_acceptance_fit": f["existing_acceptance_fit"], "existing_acceptance_relations": f["existing_acceptance_relations"]} if f.get("supplemental_acceptance_id") else {}),
                             **({"ui_action_checks": f["ui_action_checks"]} if f.get("ui_action_checks") else {})} for f in features + nonproduction],
               "supplemental_scenarios": [{"id": f["supplemental_acceptance_id"], "feature_id": f["feature_id"], "task_ids": f["task_ids"],
                                           "given": f.get("acceptance_scope", "该入口所需目标、身份和数据已准备"), "when": f.get("acceptance_when", f["user_action"]), "then": f.get("acceptance_then", f["visible_result"]),
                                           "assertions": f["acceptance_assertions"], "existing_acceptance_relations": f["existing_acceptance_relations"],
                                           **({"ui_action_checks": f["ui_action_checks"]} if f.get("ui_action_checks") else {}),
                                           **({"identity_decision": f["identity_decision"], "supported_feature_ids": f["supported_feature_ids"], "supported_scenario_ids": f["supported_scenario_ids"], "variant_group": f.get("variant_group")} if f.get("identity_decision") else {}),
                                           "formalization_task_id": f["acceptance_formalization_task_id"],
                                           "execution_task_ids": f["acceptance_execution_task_ids"], "execution_owner": f["target_owner"],
                                           "execution_stage_ids": f["stage_ids"], "due": f["acceptance_due"],
                                           "status": "SPECIFIED_NOT_EXECUTED"}
                                          for f in features if f.get("supplemental_acceptance_id")],
               "legacy_feature_id_map": legacy_id_map}
    supplemental = mapping["supplemental_scenarios"]
    if len(supplemental) != len(features) or len({s["id"] for s in supplemental}) != len(supplemental):
        raise ValueError("逐叶子场景数量或稳定 ID 不符")
    feature_by_id_for_scenario = {f["feature_id"]: f for f in features}
    for scenario in supplemental:
        feature = feature_by_id_for_scenario.get(scenario["feature_id"])
        if feature is None:
            raise ValueError(f"子场景无现役语义叶: {scenario['id']}")
        if scenario["formalization_task_id"] != "R00-T07" or not scenario["execution_task_ids"]:
            raise ValueError(f"逐叶验收未指定后续接收或执行 Task: {scenario['id']}")
        if {r["acceptance_id"] for r in scenario["existing_acceptance_relations"]} != set(feature["acceptance_ids"]):
            raise ValueError(f"逐叶验收与旧 A-ID 关系不完整: {scenario['id']}")
        if any(r["relation"] not in {"DIRECT", "INDIRECT", "GAP"} or (r["relation"] == "DIRECT") != bool(r["covers_leaf_result"]) for r in scenario["existing_acceptance_relations"]):
            raise ValueError(f"旧 A-ID 适用性与结果证明不一致: {scenario['id']}")
    semantic_branch_decisions = [
        {"review_entry_id": entry_id, "source_ref": review["source_refs"][0],
         "handler_sha256": review["reviewed_handler_sha256"], "source_sha256": review["reviewed_source_sha256"],
         "branch_signature": review.get("branch_signature"), "decision": review["decision"],
         "semantic_keys": [leaf["semantic_key"] for leaf in review_leaves(review)],
         "consumer_scope": review.get("consumer_scope", "BUILTIN_OR_INDIRECT_CALL_REVIEWED"),
         "review_confidence": review.get("confidence", "SOURCE_AND_STATIC_CONSUMER_SEARCH_REVIEWED"),
         "limitation_category": "EXTERNAL_PROTOCOL_NO_BUILTIN_CALL_FOUND" if review.get("consumer_scope") == "EXTERNAL_HTTP_NO_BUILTIN_CALL_FOUND" else "KNOWN_PRODUCT_OR_ACCEPTANCE_LIMITATION" if review.get("unresolved") else None,
         "limitations": review.get("unresolved", [])}
        for entry_id, review in sorted(semantic_reviews().items())
    ]
    coverage = {"schema_version": 2, "tested_sha": head,
                "input_roots": ["server/composition/open-root.ts", "server/composition/full-root.ts", "server/index.ts", "desktop/src/react/settings/SettingsNav.tsx", "desktop/src/react/settings/SettingsContent.tsx", "desktop/src/react/components/app/AppPages.tsx", "desktop/preload.cjs", "desktop/main.cjs", "desktop/auto-updater.cjs", "desktop/src/react/**/*", "core/slash-commands/bridge-commands.ts", "core/slash-commands/index.ts", "shared/tool-categories.ts", "hub/scheduler.ts", "lib/bridge/bridge-manager.ts", "plugins/*/manifest.json", "cli/entry.ts", "desktop/src/shared/github-release-check.cjs"],
                "registered_entry_count": len(registries), "inventory_feature_count": len(features),
                "kind_counts": {k: sum(e["kind"] == k for e in registries) for k in sorted({e["kind"] for e in registries})},
                "domain_counts": {d: sum(f["parent_domain"] == d for f in features) for d in DOMAINS},
                "differences": delta, "multi_method_decisions": multi_method_decisions,
                "side_effect_reviews": side_effect_reviews,
                "pending_semantics": pending_semantics,
                "semantic_branch_decisions": semantic_branch_decisions,
                "semantic_review_limitations": [row for row in semantic_branch_decisions
                                                if row["limitations"] or row["consumer_scope"] == "EXTERNAL_HTTP_NO_BUILTIN_CALL_FOUND"],
                "ui_projection_decisions": [{"projection": key, "owner": UI_PROJECTION_OWNER.get(key, "ui-behavior:" + key), "reason": reason} for key, reason in UI_PROJECTION_REASONS.items()],
                "body_effect_decisions": [{"entry_id": CONFIRM_ROUTE, "discriminator": "body.action", "values": list(CONFIRM_EFFECTS), "feature_ids": owner_sets[CONFIRM_ROUTE], "source_ref": ["server/routes/confirm.ts:18", "server/routes/confirm.ts:37", "desktop/src/react/components/chat/SettingsConfirmCard.tsx:98", "desktop/src/react/components/chat/SettingsConfirmCard.tsx:109"], "reason": "confirmed 与 rejected 产生相反的待处理动作结果；两叶共同引用同一生产路由"}],
                "parameter_variant_decisions": [
                    {"entry_id": "route:server-index:POST:/api/session-permission-mode", "decision": "ONE_EFFECT_SCOPED", "reason": "currentSessionOnly、pendingNewSession、sessionPath 选择不同权限作用域，但用户结果同为设置该作用域的 permissionMode；叶子文字明确四种作用域", "source_ref": ["server/index.ts:1123"]},
                    {"entry_id": "route:git-environment:POST:/git/discard", "decision": "ONE_EFFECT_SCOPED", "reason": "paths 指定文件或缺省全部已跟踪文件，均为丢弃工作树改动；叶子文字明确缺省范围", "source_ref": ["server/routes/git-environment.ts:371"]},
                    {"entry_id": "route:character-cards:POST:/character-cards/import", "decision": "ONE_EFFECT_OPTIONAL_DATA", "reason": "importMemory 控制可选记忆导入，但均提交同一个角色卡导入计划；叶子文字明确可选内容", "source_ref": ["server/routes/character-cards.ts:102"]},
                ],
                "registrations": [{"entry_id": e["entry_id"], "feature_id": owners[e["entry_id"]],
                                   "feature_ids": [*owner_sets[e["entry_id"]], *[fid for ep, fid in support_pairs if ep == e["entry_id"]]],
                                   "source_ref": e["source_ref"][0]} for e in registries]}
    validate_source_branch_audit(registries, features, coverage)
    return inventory, mapping, coverage


def negative_checks() -> None:
    # 反例只替换本进程的读取函数，不触碰仓库文件。
    original = lines
    def expect_failure(label: str, replacements: dict[str, list[str]], probe, expected: str) -> None:
        def in_memory(path: str) -> list[str]:
            return replacements.get(path, original(path))
        globals()["lines"] = in_memory
        try:
            try:
                probe()
            except ValueError as error:
                if expected not in str(error):
                    raise AssertionError(f"{label} 失败原因不符: {error}") from error
                print(f"NEGATIVE_DETECTED {label}: {error}")
            else:
                raise AssertionError(f"{label} 未被清点门禁发现")
        finally:
            globals()["lines"] = original
    expect_failure("ipc-new-handler-and-bridge", {
        "desktop/main.cjs": original("desktop/main.cjs") + ['wrapIpcHandler("r00-review-probe", () => true);'],
        "desktop/preload.cjs": original("desktop/preload.cjs") + ['r00ReviewProbe: () => ipcRenderer.invoke("r00-review-probe"),'],
    }, extract_desktop_host, "IPC 无功能归属")
    slash = original("core/slash-commands/bridge-commands.ts")
    close = next(i for i, line in enumerate(slash) if line == "];" )
    expect_failure("slash-new-core-command", {"core/slash-commands/bridge-commands.ts": slash[:close] + ['    name: "r00-review-probe",'] + slash[close:]}, extract_slash_commands, "核心 slash 注册无归属")
    mcp = original("server/routes/mcp.ts")
    changed = [line.replace('sub.post("/servers", addConnector)', 'sub.post("/servers", differentHandler)') for line in mcp]
    expect_failure("alias-handler-diverged", {"server/routes/mcp.ts": changed}, extract, "兼容地址不再指向同一 handler")
    knowledge = original("server/routes/knowledge.ts")
    expect_failure("same-url-new-effect", {
        "server/routes/knowledge.ts": knowledge + ['  route.put("/knowledge/notebooks/:id", () => ({}));'],
    }, build, "普通 route 登记集合或方法变化")
    # 注册地址未变时，源码分支及其下游 helper 的冻结签名仍须触发复审。
    def changed_source(path: str, old: str, new: str) -> dict[str, list[str]]:
        current = original(path)
        matches = [i for i, line in enumerate(current) if old in line]
        if len(matches) != 1:
            raise AssertionError(f"负例定位不唯一: {path} {old} {matches}")
        altered = current[:]
        altered[matches[0]] = altered[matches[0]].replace(old, new)
        return {path: altered}
    def audit_gate() -> None:
        validate_semantic_reviews(extract())
    for label, path, old, new in (
        ("cron-new-action", "server/routes/desk.ts", 'case "add": {', 'case "purge_all": {'),
        ("jian-null-boundary", "server/routes/desk.ts", 'content === null || content === undefined', 'content === undefined'),
        ("agent-delete-skills", "server/routes/agents.ts", 'Array.isArray(body.deleteSkills)', 'Array.isArray(body.deleteAllSkills)'),
        ("git-unstash-scope", "server/routes/git-environment.ts", 'const path = typeof body?.path', 'const path = typeof body?.scopePath'),
        ("media-capability", "server/routes/media.ts", 'capability === "speech_generation"', 'capability === "speech_plus"'),
        ("workbench-helper-safe-delete", "core/mount-aware-file-service.ts", 'async safeDelete(rootId, subdir', 'async purgeAll(rootId, subdir'),
    ):
        expect_failure(label, changed_source(path, old, new), audit_gate, "签名变化")
    saved_behavior_key = behavior_key
    for target in ("behavior:knowledge:/knowledge/notebooks/:id:", "behavior:access:/access/account/password:"):
        def wrongly_merge(item: dict) -> str:
            key = saved_behavior_key(item)
            return target + "READ" if key.startswith(target) else key
        globals()["behavior_key"] = wrongly_merge
        try:
            try:
                build()
            except ValueError as error:
                if "普通 route 登记集合或方法变化" not in str(error) and "不同效果方法被合并" not in str(error):
                    raise AssertionError(f"{target} 错合未按语义失败: {error}") from error
                print(f"NEGATIVE_DETECTED wrong-merge-{target}: {error}")
            else:
                raise AssertionError(f"{target} 错合未被清点门禁发现")
        finally:
            globals()["behavior_key"] = saved_behavior_key
    key = ("git-environment", "/git/push", "POST")
    saved_push = ROUTE_EFFECTS[key]
    ROUTE_EFFECTS[key] = ("修改 Git 推送", "Git 推送更新后可再次读取")
    try:
        expect_failure("push-side-effect-text", {}, build, "Git 推送副作用文字失真")
    finally:
        ROUTE_EFFECTS[key] = saved_push
    for wrong_owner in ("desktop-behavior:file-edit", "desktop-behavior:detached-viewer"):
        UI_PROJECTION_OWNER["file-preview"] = wrong_owner
        try:
            expect_failure("preview-wrong-owner-" + wrong_owner, {}, build, "非 HTTP 逐叶审查差集")
        finally:
            del UI_PROJECTION_OWNER["file-preview"]
    saved_tasks = SPECIAL_TASKS["ui-behavior:file-preview"]
    SPECIAL_TASKS["ui-behavior:file-preview"] = ["R04-T04"]
    try:
        expect_failure("preview-r09-removed", {}, build, "文件预览独立叶子或 R09 验收映射缺失")
    finally:
        SPECIAL_TASKS["ui-behavior:file-preview"] = saved_tasks
    saved_acceptance = PREVIEW_ACCEPTANCE[:]
    PREVIEW_ACCEPTANCE[:] = ["R09-A09"]
    try:
        expect_failure("preview-a10-removed", {}, build, "文件预览独立叶子或 R09 验收映射缺失")
    finally:
        PREVIEW_ACCEPTANCE[:] = saved_acceptance
    saved_reject = CONFIRM_EFFECTS.pop("rejected")
    try:
        expect_failure("confirm-reject-leaf-removed", {}, build, "非 HTTP 逐叶审查差集")
    finally:
        CONFIRM_EFFECTS["rejected"] = saved_reject
    saved_internal = INTERNAL_ROUTE_OWNER.pop(("server-index", "/api/health"))
    try:
        expect_failure("internal-health-independent", {}, build, "内部端点失去能力归属")
    finally:
        INTERNAL_ROUTE_OWNER[("server-index", "/api/health")] = saved_internal
    saved_projections = UI_PROJECTIONS[:]
    UI_PROJECTIONS[:] = [x for x in UI_PROJECTIONS if x[0] != "mood-live"]
    try:
        expect_failure("required-ui-projection-removed", {}, build, "无独立 URL 的受保护行为漏账")
    finally:
        UI_PROJECTIONS[:] = saved_projections
    inventory, _, coverage = build()
    # R8/R9 反例直接走源码事实门禁；候选及全部 SHA 即使同步也不能替代效果边。
    ui_baseline = json.loads((OUT / UI_ACTION_MATRIX_FILE).read_text(encoding="utf-8"))
    ui_features = {feature["feature_id"]: feature for feature in inventory["features"]}
    def ui_probe(candidate: dict) -> None:
        validate_ui_source_edges(candidate, ui_features, ROOT, lines)
    ui_probe(ui_baseline)
    print("POSITIVE_ACCEPTED r9-correct-global-and-agent-scope")
    def changed_action(action_id: str, mutate) -> dict:
        candidate = json.loads(json.dumps(ui_baseline))
        action = next(row for page in candidate["pages"] for row in page["actions"] if row["action_id"] == action_id)
        mutate(action)
        return candidate
    def expect_ui_failure(label: str, candidate: dict, expected: str) -> None:
        try:
            ui_probe(candidate)
        except ValueError as error:
            if expected not in str(error):
                raise AssertionError(f"{label} 失败原因不符: {error}") from error
            print(f"NEGATIVE_DETECTED {label}: {error}")
        else:
            raise AssertionError(f"{label} 未被源码事实门禁发现")
    local_id = next(f["feature_id"] for f in inventory["features"] if f["entry_id"] == "semantic-effect:agent.config.update")
    global_id = next(f["feature_id"] for f in inventory["features"] if f["entry_id"] == "semantic-effect:agent.config.global_update")
    local_scenario = next(f["supplemental_acceptance_id"] for f in inventory["features"] if f["entry_id"] == "semantic-effect:agent.config.update")
    global_scenario = next(f["supplemental_acceptance_id"] for f in inventory["features"] if f["entry_id"] == "semantic-effect:agent.config.global_update")
    def swap_write(action: dict, old_id: str, new_id: str, old_scenario: str, new_scenario: str) -> None:
        action["target_feature_ids"] = [new_id if fid == old_id else fid for fid in action["target_feature_ids"]]
        action["target_scenario_ids"] = [new_scenario if sid == old_scenario else sid for sid in action["target_scenario_ids"]]
        for variant in action.get("control_variants", []):
            if variant.get("target_feature_id") == old_id:
                variant["target_feature_id"] = new_id
                variant["target_scenario_id"] = new_scenario
    expect_ui_failure("r9-pure-global-wrong-local-leaf", changed_action("ui:settings:general#02", lambda a: swap_write(a, global_id, local_id, global_scenario, local_scenario)), "scope 与效果边错挂")
    expect_ui_failure("r9-pure-agent-wrong-global-leaf", changed_action("ui:settings:agent#04", lambda a: swap_write(a, local_id, global_id, local_scenario, global_scenario)), "scope 与效果边错挂")
    for label, removed_id, removed_scenario in (("r9-interface-missing-editor-global", global_id, global_scenario), ("r9-interface-missing-chat-agent", local_id, local_scenario)):
        def remove_side(action: dict, fid=removed_id, sid=removed_scenario) -> None:
            action["target_feature_ids"].remove(fid)
            action["target_scenario_ids"].remove(sid)
            action["control_variants"] = [row for row in action["control_variants"] if row.get("target_feature_id") != fid]
        expect_ui_failure(label, changed_action("ui:settings:interface#03", remove_side), "scope 与效果边错挂")
    expect_ui_failure("r9-security-text-right-fid-wrong", changed_action("ui:settings:security#03", lambda a: swap_write(a, global_id, local_id, global_scenario, local_scenario)), "scope 与效果边错挂")
    from r00_t02_source_gates import classify_config_patch
    declared_global = set(re.findall(r"^\s*['\"]?([\w.]+)['\"]?:\s*\{\s*scope:\s*['\"]global['\"]", "\n".join(lines("shared/config-schema.ts")), re.MULTILINE))
    cases = next(row["request_cases"] for row in semantic_reviews().values() if row["entry_id"] == "behavior:agents:/agents/:id/config:PUT")
    validate_config_request_cases(cases, declared_global)
    print("POSITIVE_ACCEPTED r9-pure-mixed-provider-routing")
    for label, case_id, mutate, expected in (
        ("r9-mixed-missing-global", "mixed", lambda c: c["effect_entry_ids"].remove("semantic-effect:agent.config.global_update"), "请求体与写入叶不符"),
        ("r9-mixed-missing-agent", "mixed", lambda c: c["effect_entry_ids"].remove("semantic-effect:agent.config.update"), "请求体与写入叶不符"),
        ("r9-mixed-atomic-rollback", "mixed", lambda c: c.__setitem__("failure_boundary", "失败后原子回滚所有写入"), "错误描述为原子回滚"),
        ("r9-provider-misread-as-yaml", "provider_save", lambda c: c.__setitem__("effect_entry_ids", ["semantic-effect:agent.config.update"]), "请求体与写入叶不符"),
        ("r9-inline-secret-misread-as-global-config", "inline_credential", lambda c: c.__setitem__("effect_entry_ids", ["semantic-effect:agent.config.global_update"]), "请求体与写入叶不符"),
        ("r9-other-route-provider-wrong-agents-leaf", "other_route_provider", lambda c: c.__setitem__("effect_entry_ids", ["semantic-effect:agent.config.global_update"]), "请求体与写入叶不符"),
    ):
        altered = json.loads(json.dumps(cases))
        mutate(next(case for case in altered if case["id"] == case_id))
        try:
            validate_config_request_cases(altered, declared_global)
        except ValueError as error:
            if expected not in str(error):
                raise AssertionError(f"{label} 失败原因不符: {error}") from error
            print(f"NEGATIVE_DETECTED {label}: {error}")
        else:
            raise AssertionError(f"{label} 未被配置分流门禁发现")
    print("POSITIVE_ACCEPTED r10-provider-storage-and-failure-conditions")
    for label, case_id, mutate, expected in (
        ("r10-catalog-omitted", "provider_save", lambda c: c["storage_expectation"].pop("catalog"), "存储或失败边界错挂"),
        ("r10-pure-provider-no-yaml", "provider_save", lambda c: c.__setitem__("failure_boundary", "纯 providers 请求任何 YAML 均不写"), "失败文字错挂"),
        ("r10-focus-b-misread-request-a", "provider_save", lambda c: c["storage_expectation"].__setitem__("focus_yaml", "rewrite_A_after_refresh"), "存储或失败边界错挂"),
        ("r10-plugin-unconditional", "provider_save", lambda c: c["storage_expectation"].__setitem__("plugin", "always_write"), "存储或失败边界错挂"),
        ("r10-models-unconditional", "provider_save", lambda c: c["storage_expectation"].__setitem__("models_json", "always_write"), "存储或失败边界错挂"),
        ("r10-inline-request-yaml-omitted", "inline_credential", lambda c: c["storage_expectation"].__setitem__("requested_yaml", "none"), "存储或失败边界错挂"),
        ("r10-other-route-remove-as-save", "other_route_remove", lambda c: c.__setitem__("effect_entry_ids", ["semantic-effect:provider.global.save"]), "请求体与写入叶不符"),
        ("r10-refresh-atomic-rollback", "provider_refresh_failure", lambda c: c.__setitem__("failure_boundary", "刷新失败后目录与模型原子回滚"), "失败文字错挂"),
    ):
        altered = json.loads(json.dumps(cases))
        mutate(next(case for case in altered if case["id"] == case_id))
        try:
            validate_config_request_cases(altered, declared_global)
        except ValueError as error:
            if expected not in str(error):
                raise AssertionError(f"{label} 失败原因不符: {error}") from error
            print(f"NEGATIVE_DETECTED {label}: {error}")
        else:
            raise AssertionError(f"{label} 未被供应商请求门禁发现")
    provider_reviews = {key: semantic_reviews()[key] for key in
                        ("behavior:agents:/agents/:id/config:PUT", "behavior:config:/config:PUT")}
    feature_entries = {feature["entry_id"]: feature for feature in inventory["features"]}
    validate_provider_leaf_contracts(feature_entries, provider_reviews, ROOT, lines)
    for label, entry, mutate in (
        ("r10-leaf-catalog-omitted", "semantic-effect:provider.agent.save", lambda f: f["current_stores"].pop(0)),
        ("r10-leaf-models-unconditional", "semantic-effect:provider.global.save", lambda f: f["storage_contract"].__setitem__("models_projection", "always_write")),
        ("r10-leaf-inline-yaml-omitted", "semantic-effect:provider.inline_credential.save", lambda f: f["acceptance_assertions"].pop(0)),
        ("r10-leaf-refresh-rollback", "semantic-effect:provider.global.remove", lambda f: f.__setitem__("visible_result", "刷新失败后目录原子回滚")),
    ):
        altered = json.loads(json.dumps(feature_entries))
        mutate(altered[entry])
        try:
            validate_provider_leaf_contracts(altered, provider_reviews, ROOT, lines)
        except ValueError as error:
            if "供应商逐叶库存存储或失败断言错误" not in str(error):
                raise AssertionError(f"{label} 失败原因不符: {error}") from error
            print(f"NEGATIVE_DETECTED {label}: {error}")
        else:
            raise AssertionError(f"{label} 未被逐叶库存门禁发现")
    altered_reviews = json.loads(json.dumps(provider_reviews))
    altered_leaf = next(leaf for leaf in altered_reviews["behavior:agents:/agents/:id/config:PUT"]["leaves"]
                        if leaf["semantic_key"] == "provider.agent.remove")
    altered_leaf["result"] = "纯 providers 请求任何 YAML 均不写"
    try:
        validate_provider_leaf_contracts(feature_entries, altered_reviews, ROOT, lines)
    except ValueError as error:
        if "供应商语义正源存储或失败断言错误" not in str(error):
            raise AssertionError(f"r10-source-false-result 失败原因不符: {error}") from error
        print(f"NEGATIVE_DETECTED r10-source-false-result: {error}")
    else:
        raise AssertionError("r10-source-false-result 未被语义正源门禁发现")
    def without_focus_refresh(path: str) -> list[str]:
        original_lines = lines(path)
        return [line.replace("await engine.updateConfig({})", "await engine.updateConfig({ agentId: id })")
                for line in original_lines] if path == "server/routes/agents.ts" else original_lines
    try:
        validate_provider_leaf_contracts(feature_entries, provider_reviews, ROOT, without_focus_refresh)
    except ValueError as error:
        if "供应商存储调用链源码锚点变化" not in str(error):
            raise AssertionError(f"r10-source-focus-route 失败原因不符: {error}") from error
        print(f"NEGATIVE_DETECTED r10-source-focus-route: {error}")
    else:
        raise AssertionError("r10-source-focus-route 未被现役链门禁发现")
    def altered_ui(mutator) -> dict:
        candidate = json.loads(json.dumps(ui_baseline))
        switch = next(action for page in candidate["pages"] for action in page["actions"]
                      if action["action_id"] == "ui:panel:automation#03")
        mutator(switch)
        return candidate
    enable_id = next(feature["feature_id"] for feature in inventory["features"]
                     if feature["entry_id"] == "semantic-effect:cron.update")
    disable_id = next(feature["feature_id"] for feature in inventory["features"]
                      if feature["entry_id"] == "semantic-effect:cron.toggle")
    for label, mutator, expected in (
        ("r8-enable-edge-removed-all-sha-synced", lambda action: action["target_feature_ids"].remove(enable_id), "启用 update 与停用 toggle"),
        ("r8-enable-wrongly-attached-to-toggle-all-sha-synced", lambda action: action.__setitem__("state_effect_branches", [
            {**row, "effect": "semantic-effect:cron.toggle"} if row.get("effect") == "semantic-effect:cron.update" else row
            for row in action["state_effect_branches"]]), "状态条件→本地拒绝/update/toggle"),
    ):
        try:
            ui_probe(altered_ui(mutator))
        except ValueError as error:
            if expected not in str(error):
                raise AssertionError(f"{label} 失败原因不符: {error}") from error
            print(f"NEGATIVE_DETECTED {label}: {error}")
        else:
            raise AssertionError(f"{label} 未被源码事实门禁发现")
    for label, path, old, new in (
        ("r8-enabled-callback-changed", "desktop/src/react/components/automation/AutomationCard.tsx", "onToggleEnabled(job.id);", "onUpdate(job.id, { enabled: false });"),
        ("r8-enable-draft-merge-removed", "desktop/src/react/components/automation/AutomationCard.tsx", "...updateFields(), enabled: true", "enabled: true"),
    ):
        expect_failure(label, changed_source(path, old, new), lambda: ui_probe(ui_baseline), "UI 自动化源码状态分流/草稿合并变化")
    wrong_scope = json.loads(json.dumps(ui_baseline))
    security_proxy = next(action for page in wrong_scope["pages"] for action in page["actions"]
                          if action["action_id"] == "ui:settings:security#03")
    security_proxy["storage_scope"]["global_preferences_fields"] = []
    security_proxy["storage_scope"]["agent_config_fields"] = ["network_proxy"]
    try:
        ui_probe(wrong_scope)
    except ValueError as error:
        if "持久化 scope 与请求体/CONFIG_SCHEMA 冲突" not in str(error):
            raise AssertionError(f"R8 误判全局字段归属失败原因不符: {error}") from error
        print(f"NEGATIVE_DETECTED r8-global-field-wrongly-owned-by-agent: {error}")
    else:
        raise AssertionError("R8 全局字段误判为助手所有未被源码门禁发现")
    old_result = json.loads(json.dumps(ui_baseline))
    old_page = next(page for page in old_result["pages"] if page["entry_id"] == "ui:panel:activity")
    old_page["error_empty_paths"][2]["failure_or_forbidden"] = "升级失败显示错误 toast"
    try:
        validate_ui_review_fields(old_result, nonhttp_reviews())
    except ValueError as error:
        if "UI 页面旧文字与现行动作字段冲突" not in str(error):
            raise AssertionError(f"R8 旧结果负例失败原因不符: {error}") from error
        print(f"NEGATIVE_DETECTED r8-old-page-error-text: {error}")
    else:
        raise AssertionError("R8 页面旧结果文字未被门禁发现")
    stale_reviews = json.loads(json.dumps(nonhttp_reviews()))
    stale_reviews[old_page["original_feature_id"]]["refusal_or_boundary"] = "升级失败显示错误 toast"
    try:
        validate_ui_review_fields(ui_baseline, stale_reviews)
    except ValueError as error:
        if "UI 非 HTTP 当前结果字段与逐动作候选冲突" not in str(error):
            raise AssertionError(f"R8 逐叶旧文字负例失败原因不符: {error}") from error
        print(f"NEGATIVE_DETECTED r8-old-review-result-text: {error}")
    else:
        raise AssertionError("R8 逐叶旧结果文字未被门禁发现")
    frozen_registries = extract()
    def unified_gate() -> None:
        validate_source_branch_audit(frozen_registries, inventory["features"], coverage)
    confirm = original("server/routes/confirm.ts")
    confirm_line = next(i for i, line in enumerate(confirm) if 'if (!action || !["confirmed", "rejected"].includes(action))' in line)
    expect_failure("confirm-deferred-before-original-check", {
        "server/routes/confirm.ts": confirm[:confirm_line] + ['    if (action === "deferred") return c.json({ ok: true, deferred: true });'] + confirm[confirm_line:],
    }, unified_gate, "签名变化")
    for label, path, old, new in (
        ("static-route-new-mode", "server/routes/git-environment.ts", 'route.post("/git/push", async (c) => {', 'route.post("/git/push", async (c) => {\n    if (c.req.query("mode") === "purge") return c.json({ ok: true });'),
        ("internal-log-new-effect", "server/index.ts", 'app.post("/api/log", async (c) => {', 'app.post("/api/log", async (c) => {\n    if (c.req.query("mode") === "external") return c.json({ sent: true });'),
        ("ask-user-fourth-choice", "lib/tools/ask-user-tool.ts", 'if (action === "confirmed") {', 'if (action === "skipped") return toolError("skipped");\n      if (action === "confirmed") {'),
        ("beautify-new-generation", "plugins/beautify/tools/create-cover.ts", 'const generatedFilePath = resolveGeneratedFilePath(input);', 'const generatedFilePath = resolveGeneratedFilePath(input) || await ctx.generateImage?.(input);'),
        ("ipc-write-overwrite-branch", "desktop/main.cjs", 'return writeTextFileIfUnchanged(filePath, content, expectedVersion || null);', 'if (expectedVersion === "overwrite") return fs.writeFileSync(filePath, content, "utf8");\n    return writeTextFileIfUnchanged(filePath, content, expectedVersion || null);'),
        ("slash-reject-approves", "core/slash-commands/bridge-commands.ts", 'handler: async (ctx) => _resolvePendingConfirmation(ctx, "rejected"),', 'handler: async (ctx) => _resolvePendingConfirmation(ctx, "confirmed"),'),
        ("confirm-card-consumer-removed", "desktop/src/react/components/chat/SettingsConfirmCard.tsx", "body: JSON.stringify({ action: 'rejected' }),", "body: JSON.stringify({ action: 'confirmed' }),"),
        ("request-authorization-changed", "server/http/boundary.ts", "authorize(capability, target = {}) {", "authorize(capability, target = {}) {\n      return { allowed: true };"),
    ):
        expect_failure(label, changed_source(path, old, new), unified_gate,
                       "ask_user 源码分支与人工语义契约差集" if label == "ask-user-fourth-choice" else "签名变化")
    # 只更新逐叶报告的源码摘要，未重审分支/结果/断言时，统一审查身份仍应过期。
    ask_review = next(row for row in nonhttp_reviews().values() if row["entry_id"] == "tool:ask_user")
    ask_source = "lib/tools/ask-user-tool.ts"
    old_hash = ask_review["reviewed_source_sha256"][ask_source]
    mutated = changed_source(ask_source, 'if (action === "confirmed") {', 'if (action === "skipped") return toolError("skipped");\n      if (action === "confirmed") {')
    ask_review["reviewed_source_sha256"][ask_source] = hashlib.sha256("\n".join(mutated[ask_source]).encode()).hexdigest()
    try:
        expect_failure("digest-only-without-new-decision", mutated, unified_gate, "ask_user 源码分支与人工语义契约差集")
    finally:
        ask_review["reviewed_source_sha256"][ask_source] = old_hash
    saved_scenario = ask_review["scenario"]
    ask_review["scenario"] = None
    try:
        try:
            validate_nonhttp_review(next(f for f in inventory["features"] if f["feature_id"] == ask_review["original_feature_id"]), ask_review, {f["feature_id"] for f in inventory["features"]})
        except ValueError as error:
            if "缺少适用场景" not in str(error):
                raise AssertionError(f"ask_user 场景删除失败原因不符: {error}") from error
            print(f"NEGATIVE_DETECTED ask-user-scenario-removed: {error}")
        else:
            raise AssertionError("ask_user 场景删除未被门禁发现")
    finally:
        ask_review["scenario"] = saved_scenario
    saved_relation = ask_review["old_acceptance_relations"][0]["id"]
    ask_review["old_acceptance_relations"][0]["id"] = "R07-A09"
    try:
        try:
            validate_nonhttp_review(next(f for f in inventory["features"] if f["feature_id"] == ask_review["original_feature_id"]), ask_review, {f["feature_id"] for f in inventory["features"]})
        except ValueError as error:
            if "旧 A-ID 未逐项裁决" not in str(error):
                raise AssertionError(f"ask_user 错挂 A-ID 失败原因不符: {error}") from error
            print(f"NEGATIVE_DETECTED ask-user-wrong-old-AID: {error}")
        else:
            raise AssertionError("ask_user 错挂 A-ID 未被门禁发现")
    finally:
        ask_review["old_acceptance_relations"][0]["id"] = saved_relation
    provider_review = next(row for row in nonhttp_reviews().values() if row["entry_id"] == "provider:deepseekResponses")
    saved_provider_result = provider_review["success_result"]
    provider_review["success_result"] = "目录显示模型即代表真实调用已通过"
    try:
        try:
            validate_nonhttp_review(next(f for f in inventory["features"] if f["feature_id"] == provider_review["original_feature_id"]), provider_review, {f["feature_id"] for f in inventory["features"]})
        except ValueError as error:
            if "供应商目录项不能冒称真实调用已验" not in str(error):
                raise AssertionError(f"供应商目录冒称运行验证失败原因不符: {error}") from error
            print(f"NEGATIVE_DETECTED provider-directory-false-live-claim: {error}")
        else:
            raise AssertionError("供应商目录冒称运行验证未被门禁发现")
    finally:
        provider_review["success_result"] = saved_provider_result
    # 三类 R5 反例都执行完整 build()，并在内存中同步需要的摘要；候选文件不改动。
    global SOURCE_BRANCH_AUDIT_OVERRIDE
    original_unified = json.loads((OUT / SOURCE_BRANCH_AUDIT_FILE).read_text(encoding="utf-8"))
    ask_source = "lib/tools/ask-user-tool.ts"
    ask_review = next(row for row in nonhttp_reviews().values() if row["entry_id"] == "tool:ask_user")
    ask_fid = ask_review["original_feature_id"]
    ask_old_hash = ask_review["reviewed_source_sha256"][ask_source]
    new_ask_source = changed_source(ask_source, 'if (action === "confirmed") {', 'if (action === "skipped") return toolError("skipped");\n      if (action === "confirmed") {')
    ask_review["reviewed_source_sha256"][ask_source] = hashlib.sha256("\n".join(new_ask_source[ask_source]).encode()).hexdigest()
    synchronized = json.loads(json.dumps(original_unified))
    ask_row = next(row for row in synchronized["reviews"] if row["entry_id"] == "tool:ask_user")
    ask_row["reviewed_source_sha256"][ask_source] = ask_review["reviewed_source_sha256"][ask_source]
    ask_row["audit_record_sha256"][ask_fid] = review_record_sha256(ask_review)
    SOURCE_BRANCH_AUDIT_OVERRIDE = synchronized
    try:
        expect_failure("r5-full-build-ask-user-two-digests-synced", new_ask_source, build,
                       "ask_user 源码分支与人工语义契约差集")
    finally:
        ask_review["reviewed_source_sha256"][ask_source] = ask_old_hash
        SOURCE_BRANCH_AUDIT_OVERRIDE = None
    # AST 判定只依赖真实 decision.action 来源。同步逐叶、统一和汇总摘要后，
    # 等价条件出现的新值仍须触发人工裁决；同名无关对象不得误报。
    from r00_t02_source_gates import ask_user_source_branches
    contract_values = ["confirmed", "timeout", "aborted", "<default>"]
    ast_cases = {
        "reverse-strict-equality": 'if ("skipped" === action) return toolError("skipped");\n      ',
        "inequality-else-return": 'if (action !== "skipped") {} else return toolError("skipped");\n      ',
        "negated-boolean-composition": 'if (!(action !== "skipped") && true) return toolError("skipped");\n      ',
        "array-includes": 'if (["skipped"].includes(action)) return toolError("skipped");\n      ',
        "constant-set": 'const allowed = new Set(["skipped"]);\n      if (allowed.has(action)) return toolError("skipped");\n      ',
        "constant-scalar-alias": 'const skippedAction = "skipped";\n      if (action === skippedAction) return toolError("skipped");\n      ',
        "local-helper": 'const isSkipped = (x: string) => x === "skipped";\n      if (isSkipped(action)) return toolError("skipped");\n      ',
        "alias-early-return": 'const skipped = action === "skipped";\n      if (skipped) return toolError("skipped");\n      ',
        "same-flags-other-message": 'if (action === "skipped") return toolOk("new", {answered:false,dismissed:true,answers:[]});\n      ',
        "switch-default-first": 'switch(action){default: return toolOk("old",{answered:false,dismissed:true,answers:[]});case "skipped":return toolError("skipped");}\n      ',
    }
    for label, prefix in ast_cases.items():
        mutation = changed_source(ask_source, 'if (action === "confirmed") {', prefix + 'if (action === "confirmed") {')
        found = ask_user_source_branches(ROOT, lambda path: mutation.get(path, original(path)), contract_values)
        if "skipped" not in found:
            raise AssertionError(f"AST {label} 未发现 skipped")
        print(f"AST_DETECTED {label}: {found['skipped']}")
    dynamic = changed_source(ask_source, 'if (action === "confirmed") {',
                             'if (allowedActions.includes(action)) return toolError("skipped");\n      if (action === "confirmed") {')
    try:
        ask_user_source_branches(ROOT, lambda path: dynamic.get(path, original(path)), contract_values)
    except ValueError as error:
        if "MANUAL" not in str(error):
            raise AssertionError(f"AST 未知集合失败原因不符: {error}") from error
        print("AST_MANUAL dynamic-action-collection")
    else:
        raise AssertionError("AST 未知集合未阻止自动通过")
    mutable = changed_source(ask_source, 'if (action === "confirmed") {',
                             'const allowed = new Set(["skipped"]); allowed.add(runtimeValue); if (allowed.has(action)) return toolError("skipped");\n      if (action === "confirmed") {')
    try:
        ask_user_source_branches(ROOT, lambda path: mutable.get(path, original(path)), contract_values)
    except ValueError as error:
        if "MANUAL" not in str(error):
            raise AssertionError(f"AST 可变 Set 失败原因不符: {error}") from error
        print("AST_MANUAL mutated-action-set")
    else:
        raise AssertionError("AST 可变 Set 未阻止自动通过")
    unknown_case = changed_source(ask_source, 'if (action === "confirmed") {',
                                  'switch(action) { case runtimeAction: return toolError("unknown"); default: break; }\n      if (action === "confirmed") {')
    try:
        ask_user_source_branches(ROOT, lambda path: unknown_case.get(path, original(path)), contract_values)
    except ValueError as error:
        if "MANUAL" not in str(error):
            raise AssertionError(f"AST 动态 switch case 失败原因不符: {error}") from error
        print("AST_MANUAL dynamic-switch-case")
    else:
        raise AssertionError("AST 动态 switch case 未阻止自动通过")
    unrelated = changed_source(ask_source, 'if (action === "confirmed") {',
                               'const unrelated = { action: "skipped" }; if (unrelated.action === "skipped") console.log("x");\n      if (action === "confirmed") {')
    if ask_user_source_branches(ROOT, lambda path: unrelated.get(path, original(path)), contract_values) != ask_user_source_branches(ROOT, original, contract_values):
        raise AssertionError("同名无关 action 被 AST 误判成选择分支")
    print("AST_UNRELATED_ACTION_IGNORED")
    shadow = changed_source(ask_source, 'if (action === "confirmed") {',
                            '{ const action = "skipped"; if (action === "skipped") console.log("x"); }\n      if (action === "confirmed") {')
    if ask_user_source_branches(ROOT, lambda path: shadow.get(path, original(path)), contract_values) != ask_user_source_branches(ROOT, original, contract_values):
        raise AssertionError("词法内层同名 action 被误判成外层选择")
    print("AST_SHADOW_ACTION_IGNORED")
    from r00_t02_source_gates import validate_ask_user_contract
    adjudicated = json.loads((OUT / BRANCH_CONTRACT_FILE).read_text(encoding="utf-8"))
    equivalent = changed_source(ask_source, 'if (action === "confirmed") {', 'if (["confirmed"].includes(action)) {')
    discovery = ask_user_source_branches(ROOT, lambda path: equivalent.get(path, original(path)), contract_values, include_discovery=True)
    adjudicated["source_condition_digest"] = discovery["condition_digest"]
    adjudicated["branches"][0]["source_condition"] = '["confirmed"].includes(action)，人工裁决与原 confirmed 比较等价'
    validate_ask_user_contract(adjudicated, ask_review, ask_review["scenario"], ROOT,
                               lambda path: equivalent.get(path, original(path)))
    print("AST_EQUIVALENT_REWRITE_AFTER_ADJUDICATION_OK")
    changed_message = changed_source(ask_source, 'approval.askUser.answered', 'approval.askUser.dismissed')
    try:
        validate_ask_user_contract(json.loads((OUT / BRANCH_CONTRACT_FILE).read_text(encoding="utf-8")),
                                   ask_review, ask_review["scenario"], ROOT,
                                   lambda path: changed_message.get(path, original(path)))
    except ValueError as error:
        if "源码结果依赖" not in str(error):
            raise AssertionError(f"AST 结果依赖失败原因不符: {error}") from error
        print("AST_RESULT_DEPENDENCY_CHANGE_DETECTED")
    else:
        raise AssertionError("AST 结果文案依赖变化未被门禁发现")
    for label in ("array-includes", "same-flags-other-message"):
        mutation = changed_source(ask_source, 'if (action === "confirmed") {', ast_cases[label] + 'if (action === "confirmed") {')
        new_hash = hashlib.sha256("\n".join(mutation[ask_source]).encode()).hexdigest()
        ask_review["reviewed_source_sha256"][ask_source] = new_hash
        synchronized = json.loads(json.dumps(original_unified))
        ask_row = next(row for row in synchronized["reviews"] if row["entry_id"] == "tool:ask_user")
        ask_row["reviewed_source_sha256"][ask_source] = new_hash
        ask_row["audit_record_sha256"][ask_fid] = review_record_sha256(ask_review)
        SOURCE_BRANCH_AUDIT_OVERRIDE = synchronized
        try:
            expect_failure("r6-full-build-" + label + "-all-digests-synced", mutation, build,
                           "ask_user 源码分支与人工语义契约差集")
        finally:
            ask_review["reviewed_source_sha256"][ask_source] = ask_old_hash
            SOURCE_BRANCH_AUDIT_OVERRIDE = None
    panel = "desktop/src/react/settings/tabs/observability/ObservabilityUsagePanel.tsx"
    expect_failure("r5-full-build-final-consumer-assignment", changed_source(panel, "setAggregate(result);", "setAggregate(null);"),
                   build, "源码分支签名变化")
    usage_review = next(row for row in nonhttp_reviews().values() if row["entry_id"] == "ui:settings:usage")
    old_support = usage_review["supported_feature_ids"][:]
    usage_review["supported_feature_ids"] = [make_id("D22", "DESKTOP_BEHAVIOR", "desktop-behavior:observability-export")]
    synchronized = json.loads(json.dumps(original_unified))
    usage_row = next(row for row in synchronized["reviews"] if row["entry_id"] == "ui:settings:usage")
    usage_row["audit_record_sha256"][usage_review["original_feature_id"]] = review_record_sha256(usage_review)
    SOURCE_BRANCH_AUDIT_OVERRIDE = synchronized
    try:
        expect_failure("r5-full-build-existing-wrong-support-target", {}, build, "UI 支持目标与逐动作调用链不一致")
    finally:
        usage_review["supported_feature_ids"] = old_support
        SOURCE_BRANCH_AUDIT_OVERRIDE = None
    by_entry = {r["entry_id"]: r["feature_id"] for r in coverage["registrations"]}
    for internal in ("route:server-index:GET:/api/health", "route:server-index:POST:/api/log", "route:mcp:GET:/oauth/callback"):
        if internal not in by_entry:
            raise AssertionError(f"内部端点丢失: {internal}")
        if any(x["feature_id"] == by_entry[internal] and x["entry_id"] == internal for x in inventory["features"]):
            raise AssertionError(f"内部端点被独立计为路由叶子: {internal}")
    print("INTERNAL_ENDPOINTS_ATTACHED_AND_NOT_INDEPENDENT")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--negative-checks", action="store_true")
    args = parser.parse_args()
    outputs = dict(zip(["FEATURE_INVENTORY.json", "FEATURE_STAGE_ACCEPTANCE.json", "ENTRYPOINT_COVERAGE.json"], build()))
    for name, data in outputs.items():
        rendered = json.dumps(data, ensure_ascii=False, indent=2) + "\n"
        target = OUT / name
        if args.write:
            target.write_text(rendered, encoding="utf-8")
        elif not target.exists() or target.read_text(encoding="utf-8") != rendered:
            raise SystemExit(f"STALE: {name}; run python3 -B docs/rust-tauri/R00/r00_t02_inventory.py --write")
    coverage = outputs["ENTRYPOINT_COVERAGE.json"]
    if any(coverage["differences"].values()) or any(n == 0 for n in coverage["domain_counts"].values()):
        raise SystemExit("FAIL: orphan registration, inventory-only item, duplicate ID, or empty domain")
    if args.negative_checks:
        negative_checks()
    print(json.dumps({"result": "CHECKS_OK", "entries": coverage["registered_entry_count"], "domains": len(DOMAINS), "differences": coverage["differences"]}, ensure_ascii=False))


if __name__ == "__main__":
    main()
