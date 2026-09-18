/**
 * 环境依赖登记表（纯数据，无 I/O）。
 *
 * 每一行描述一个「本机二进制/运行时」或「应用按需下载的托管二进制」：
 * - binary/runtime 类：从 PATH 探测（execFile --version）。
 * - managed 类：应用按需下载到托管目录（与 ripgrep/fd 同一机制），
 *   探测时先查托管目录，再回落 PATH。
 *
 * projectSignals 用于「项目需要但没装」的推断：工作区里出现这些信号文件，
 * 就认为当前项目需要该依赖（如 go.mod → gopls）。
 *
 * requiredBy 是面向用户的功能名 id，前端翻译成人话（哪些功能会受影响）。
 */

export type EnvDepKind = "binary" | "runtime" | "managed";

export interface EnvDepEntry {
  id: string;
  /** 展示名（不翻译，二进制名字本身就是通用名） */
  label: string;
  kind: EnvDepKind;
  /** 探测候选命令，按顺序尝试，第一个成功即算已安装 */
  binaries: string[];
  /** 取版本号的参数 */
  versionArgs: string[];
  /** 托管二进制的文件名（不含扩展名；kind=managed 时必填） */
  managedBinName?: string;
  /** 依赖它的功能 id 列表（前端 i18n 键后缀） */
  requiredBy: string[];
  /** 工作区信号文件（精确文件名或 *.ext 后缀模式） */
  projectSignals?: string[];
  /** 安装指引（按平台给命令文本；前端原样展示为代码） */
  installHint?: { darwin?: string; win32?: string; linux?: string; other?: string };
}

export const ENV_DEP_ENTRIES: readonly EnvDepEntry[] = [
  {
    id: "git",
    label: "Git",
    kind: "binary",
    binaries: ["git"],
    versionArgs: ["--version"],
    requiredBy: ["workspace_snapshots", "file_history"],
    installHint: {
      darwin: "xcode-select --install",
      win32: "winget install Git.Git",
      linux: "sudo apt install git",
    },
  },
  {
    id: "python3",
    label: "Python 3",
    kind: "runtime",
    binaries: process.platform === "win32" ? ["python", "python3"] : ["python3", "python"],
    versionArgs: ["--version"],
    requiredBy: ["run_code"],
    projectSignals: ["*.py", "requirements.txt", "pyproject.toml", "Pipfile"],
    installHint: {
      darwin: "brew install python@3",
      win32: "winget install Python.Python.3.12",
      linux: "sudo apt install python3",
    },
  },
  {
    id: "node",
    label: "Node.js",
    kind: "runtime",
    binaries: ["node"],
    versionArgs: ["--version"],
    requiredBy: ["run_code"],
    projectSignals: ["package.json"],
    installHint: {
      darwin: "brew install node",
      win32: "winget install OpenJS.NodeJS.LTS",
      linux: "sudo apt install nodejs",
    },
  },
  {
    id: "ripgrep",
    label: "ripgrep",
    kind: "managed",
    binaries: ["rg"],
    versionArgs: ["--version"],
    managedBinName: "rg",
    requiredBy: ["grep_tool"],
  },
  {
    id: "fd",
    label: "fd",
    kind: "managed",
    binaries: ["fd"],
    versionArgs: ["--version"],
    managedBinName: "fd",
    requiredBy: ["find_tool"],
  },
  {
    id: "ast_grep",
    label: "ast-grep",
    kind: "managed",
    binaries: ["sg", "ast-grep"],
    versionArgs: ["--version"],
    managedBinName: "ast-grep",
    requiredBy: ["ast_tools"],
    installHint: {
      darwin: "brew install ast-grep",
      win32: "winget install ast-grep.ast-grep",
      linux: "cargo install ast-grep",
    },
  },
  {
    id: "semgrep",
    label: "Semgrep",
    kind: "binary",
    binaries: ["semgrep"],
    versionArgs: ["--version"],
    requiredBy: ["security_scan"],
    installHint: {
      darwin: "brew install semgrep",
      win32: "pip install semgrep",
      linux: "pip install semgrep",
    },
  },
  {
    id: "gitleaks",
    label: "Gitleaks",
    kind: "binary",
    binaries: ["gitleaks"],
    versionArgs: ["version"],
    requiredBy: ["security_scan"],
    installHint: {
      darwin: "brew install gitleaks",
      win32: "winget install gitleaks",
      linux: "sudo apt install gitleaks",
    },
  },
  {
    id: "typescript",
    label: "TypeScript (tsc)",
    kind: "binary",
    binaries: ["tsc"],
    versionArgs: ["--version"],
    requiredBy: ["lsp"],
    projectSignals: ["tsconfig.json", "*.ts", "*.tsx"],
    installHint: {
      other: "npm install -g typescript",
    },
  },
  {
    id: "pyright",
    label: "Pyright",
    kind: "binary",
    binaries: ["pyright"],
    versionArgs: ["--version"],
    requiredBy: ["lsp"],
    projectSignals: ["*.py", "pyproject.toml"],
    installHint: {
      other: "npm install -g pyright",
    },
  },
  {
    id: "gopls",
    label: "gopls",
    kind: "binary",
    binaries: ["gopls"],
    versionArgs: ["version"],
    requiredBy: ["lsp"],
    projectSignals: ["go.mod"],
    installHint: {
      other: "go install golang.org/x/tools/gopls@latest",
    },
  },
  {
    id: "rust_analyzer",
    label: "rust-analyzer",
    kind: "binary",
    binaries: ["rust-analyzer"],
    versionArgs: ["--version"],
    requiredBy: ["lsp"],
    projectSignals: ["Cargo.toml"],
    installHint: {
      other: "rustup component add rust-analyzer",
    },
  },
  {
    id: "clangd",
    label: "clangd",
    kind: "binary",
    binaries: ["clangd"],
    versionArgs: ["--version"],
    requiredBy: ["lsp"],
    projectSignals: ["*.c", "*.cpp", "*.cc", "compile_commands.json"],
    installHint: {
      darwin: "brew install llvm",
      win32: "winget install LLVM.LLVM",
      linux: "sudo apt install clangd",
    },
  },
];

export function getEnvDepEntry(id: string): EnvDepEntry | null {
  return ENV_DEP_ENTRIES.find(e => e.id === id) ?? null;
}
