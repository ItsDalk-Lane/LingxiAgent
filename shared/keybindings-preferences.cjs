/**
 * 快捷键绑定偏好（keybindings）。
 *
 * 单一事实源：所有可配置快捷键命令在这里注册（id / 默认键位 / 作用域），
 * 渲染层 dispatcher、设置页快捷键标签页、主进程全局注册三方都从这份表出发，
 * 避免键位散落在各自文件里造成三处口径漂移。
 *
 * 偏好存储语义（prefs.keybindings）：
 * - 只存「显式覆盖项」。某个命令的键位等于默认值时不落盘，删除覆盖即恢复默认。
 * - 每条命令的值是字符串数组（最多 3 组），空数组 = 显式未绑定。
 *
 * 作用域（scope）三级：
 * - global：系统级（Electron globalShortcut），应用不在前台也可触发；
 * - app：应用内生效，主窗口统一 dispatcher 分发；
 * - local：仅在特定界面聚焦时生效（如语音录制只认主聊天输入区）。
 */

const DEFAULT_MAX_BINDINGS_PER_COMMAND = 3;

/**
 * 内置快捷键命令注册表。新增命令时：
 * 1. 这里加一条；2. 渲染层 registry（desktop/src/react/keybindings/commands.ts）
 * 补文案 key；3. 需要分发目标的命令在 app-init 的 handlers 里接上。
 * quick-chat.toggle 特殊：由主进程 globalShortcut 注册，不经过渲染层 dispatcher。
 */
const KEYBINDING_COMMANDS = [
  { id: "quick-chat.toggle", defaultKeys: ["Alt+Space"], scope: "global" },
  { id: "app.restart", defaultKeys: ["CommandOrControl+Alt+R"], scope: "app" },
  { id: "app.open-settings", defaultKeys: ["CommandOrControl+,"], scope: "app" },
  { id: "app.new-session", defaultKeys: ["CommandOrControl+N"], scope: "app" },
  { id: "app.toggle-sidebar", defaultKeys: ["CommandOrControl+Shift+S"], scope: "app" },
  { id: "voice.record-toggle", defaultKeys: ["CommandOrControl+Shift+M"], scope: "local" },
];

const KEYBINDING_COMMAND_IDS = KEYBINDING_COMMANDS.map((c) => c.id);
const KEYBINDING_SCOPES = ["global", "app", "local"];

function defaultKeybindings() {
  const out = {};
  for (const command of KEYBINDING_COMMANDS) {
    out[command.id] = command.defaultKeys.slice();
  }
  return out;
}

const MODIFIER_ALIASES = {
  shift: "Shift",
  control: "Control",
  ctrl: "Control",
  alt: "Alt",
  option: "Alt",
  meta: "Meta",
  cmd: "Command",
  command: "Command",
  commandorcontrol: "CommandOrControl",
  cmdorctrl: "CommandOrControl",
  ctrlorcommand: "CommandOrControl",
};

/** 归一单组键位字符串（与 quick-chat-preferences 的 normalizeShortcut 同规则，另加修饰符别名与单字母大写归一）。 */
function normalizeBinding(value) {
  if (typeof value !== "string") return "";
  // NBSP 先替换成普通空格：value.trim() 会把末尾 NBSP 连同 Space 键一起吞掉
  const raw = String(value).replace(/\u00A0/g, " ");
  if (!raw.trim()) return "";
  return raw
    .split("+")
    .map((part) => {
      const original = String(part ?? "");
      if (original === "Spacebar") return "Space";
      // 纯空白 part 视为 Space 键
      if (original.length > 0 && original.trim() === "") return "Space";
      const trimmed = original.trim();
      if (!trimmed) return "";
      // 修饰符别名与大小写归一（用户可能手改 preferences.json）
      const alias = MODIFIER_ALIASES[trimmed.toLowerCase()];
      if (alias) return alias;
      if (trimmed.toLowerCase() === "space") return "Space";
      if (trimmed === "Esc") return "Escape";
      // 单字母键统一大写（Electron accelerator 约定 A-Z）
      return trimmed.length === 1 ? trimmed.toUpperCase() : trimmed;
    })
    .filter(Boolean)
    .join("+");
}

/**
 * 归一显式覆盖记录：只保留注册表内的命令，键位数组去重、限长；
 * 等于默认键位的覆盖项剔除（存储最小化，删覆盖即恢复默认）。
 */
function normalizeKeybindings(value = {}) {
  const source = value && typeof value === "object" && !Array.isArray(value) ? value : {};
  const out = {};
  for (const command of KEYBINDING_COMMANDS) {
    const raw = source[command.id];
    if (raw === undefined || raw === null) continue;
    const list = Array.isArray(raw) ? raw : [raw];
    const normalized = [];
    for (const item of list) {
      const binding = normalizeBinding(item);
      if (binding && !normalized.includes(binding)) normalized.push(binding);
    }
    const truncated = normalized.slice(0, DEFAULT_MAX_BINDINGS_PER_COMMAND);
    const sameAsDefault =
      truncated.length === command.defaultKeys.length &&
      truncated.every((key, index) => key === command.defaultKeys[index]);
    if (sameAsDefault) continue; // 显式默认值不落盘
    out[command.id] = truncated;
  }
  return out;
}

function mergeKeybindings(stored = {}, patch = {}) {
  const base = normalizeKeybindings(stored);
  const incoming = normalizeKeybindings(patch);
  return normalizeKeybindings({ ...base, ...incoming });
}

/**
 * 计算命令的有效键位表：显式覆盖优先，未覆盖的命令回落默认值。
 * 返回 Record<commandId, string[]>，注册表内每条命令都有键。
 */
function getEffectiveKeybindings(stored = {}) {
  const overrides = normalizeKeybindings(stored);
  const out = {};
  for (const command of KEYBINDING_COMMANDS) {
    out[command.id] = overrides[command.id] ?? command.defaultKeys.slice();
  }
  return out;
}

/**
 * 冲突检测：给定的键位组是否已被「其他命令」绑定。
 * 返回冲突的命令 id 列表（空数组 = 无冲突）。
 */
function findKeybindingConflicts(commandId, bindings, stored = {}) {
  const effective = getEffectiveKeybindings(stored);
  const candidates = (Array.isArray(bindings) ? bindings : [bindings]).filter(Boolean);
  const conflicts = [];
  for (const [id, keys] of Object.entries(effective)) {
    if (id === commandId) continue;
    if (keys.some((key) => candidates.includes(key))) conflicts.push(id);
  }
  return conflicts;
}

module.exports = {
  DEFAULT_MAX_BINDINGS_PER_COMMAND,
  KEYBINDING_COMMANDS,
  KEYBINDING_COMMAND_IDS,
  KEYBINDING_SCOPES,
  defaultKeybindings,
  normalizeBinding,
  normalizeKeybindings,
  mergeKeybindings,
  getEffectiveKeybindings,
  findKeybindingConflicts,
};
