/**
 * secret-rules.ts — security_scan 基线规则表（阶段二·9）。
 *
 * 结构照抄 lib/security/injection-scan.ts 的规则三元组（ruleId/severity/
 * pattern），正则单一事实源自 shared/log-redactor 与 lib/pii-guard 的现成
 * 脱敏模式（导出复用，不复制第二份）。分级三级：critical（确定密钥/私钥）、
 * warning（疑似密钥泄漏/敏感数据）、info（个人敏感信息，按需处理）。
 * 命中永远只带 ruleId+行号，不带命中正文（照 injection-scan 的边界手法）。
 */
import { API_KEY_VALUE_RE, SECRET_ASSIGN_RE, URL_SECRET_QUERY_RE } from "../../shared/log-redactor.ts";
import { HARD_PATTERNS } from "../pii-guard.ts";

export type SecretSeverity = "critical" | "warning" | "info";

export interface SecretRule {
  ruleId: string;
  severity: SecretSeverity;
  pattern: RegExp;
  /** 结果展示用的一句话说明（不含命中内容）。 */
  note: string;
}

/** pii-guard 的具名规则 → security_scan 分级映射。 */
const PII_SEVERITY: Record<string, SecretSeverity> = {
  private_key: "critical",
  api_key: "critical",
  credit_card: "warning",
  id_card: "info",
  ssn: "info",
};

export const SECRET_RULES: ReadonlyArray<SecretRule> = [
  {
    ruleId: "secret_api_key_literal",
    severity: "critical",
    pattern: API_KEY_VALUE_RE,
    note: "Recognized provider API key literal (openai/aws/groq/github/gitlab/slack families)",
  },
  ...HARD_PATTERNS
    .filter(({ name }) => PII_SEVERITY[name] && name !== "api_key")
    .map(({ name, regex }) => ({
      ruleId: `pii_${name}`,
      severity: PII_SEVERITY[name],
      pattern: regex,
      note: `PII guard hard pattern: ${name}`,
    })),
  {
    ruleId: "secret_assignment",
    severity: "warning",
    pattern: SECRET_ASSIGN_RE,
    note: "key/token/password assignment with an inline value (heuristic — review for real secrets)",
  },
  {
    ruleId: "secret_url_query",
    severity: "warning",
    pattern: URL_SECRET_QUERY_RE,
    note: "secret-like query parameter in a URL",
  },
];

export interface SecretFinding {
  file: string;
  ruleId: string;
  severity: SecretSeverity;
  line: number;
}

/** 把一段文本按规则表扫描；只返回 ruleId+行号，不带正文。 */
export function scanTextForSecrets(text: string): Array<{ ruleId: string; severity: SecretSeverity; line: number }> {
  if (!text) return [];
  const hits: Array<{ ruleId: string; severity: SecretSeverity; line: number }> = [];
  for (const rule of SECRET_RULES) {
    const re = new RegExp(rule.pattern.source, rule.pattern.flags.includes("g") ? rule.pattern.flags : rule.pattern.flags + "g");
    let m: RegExpExecArray | null;
    let guard = 0;
    while ((m = re.exec(text)) !== null && guard < 200) {
      guard += 1;
      if (m[0].length === 0) { re.lastIndex += 1; continue; }
      const line = text.slice(0, m.index).split("\n").length;
      hits.push({ ruleId: rule.ruleId, severity: rule.severity, line });
      if (guard >= 200) break;
    }
  }
  // 同规则同行去重（一行多个匹配只报一次）
  const seen = new Set<string>();
  return hits.filter((h) => {
    const key = `${h.ruleId}:${h.line}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}
