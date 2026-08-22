/**
 * Phase 6 Redaction Contract 单元测试（§二十三～§三十七/§三十八～§四十三/
 * §一百二十七～§一百三十三）。
 *
 * 毒丸正例（必须被替换/外置）+ 反例（普通文本必须存活）+ 原对象不可变 +
 * 资源上限显式截断 + 循环引用 + span remap。全部纯函数级。
 */
import { describe, expect, it } from "vitest";
import {
  INLINE_SECRET_PLACEHOLDER,
  isCredentialKey,
  looksLikeLocalPath,
  PROVIDER_BODY_CREDENTIAL_PATHS,
  redactTextWithMap,
  remapSpanAfterRedaction,
  sanitizeCapturedUrl,
  sanitizeValueForCapture,
  secretPathsForProtocol,
} from "../lib/llm/model-call-payload-redaction.ts";
import { MODEL_CALL_PAYLOAD_CAPTURE_LIMITS } from "../lib/llm/model-call-payload-types.ts";

/* ── 毒丸常量（§一百二十七）────────────────────────────────────────── */

const POISONS = {
  apiKey: "sk-TOPSECRET-API-KEY-A1B2C3D4E5F6",
  bearer: "TOPSECRET_AUTH_BEARER_9Z8Y7X6W5V4U3T",
  cookie: "TOPSECRET_COOKIE_SESSION=deadbeefcafe",
  oauthRefresh: "TOPSECRET_OAUTH_REFRESH_abcdefgh1234567890",
  volcengineUid: "TOPSECRET_VOLCENGINE_UID",
  signedUrlSecret: "X-Amz-Signature=TOPSECRET-SIGNED-URL-9f8e7d6c",
  localPath: "/Users/taro/TOPSECRET_LOCAL_PATH/secret.png",
  privateKey: [
    "-----BEGIN RSA PRIVATE KEY-----",
    "MIIEpTOPSECRET_PRIVATE_KEY_ThisIsNotARealKeyButPoisonForTests0123456789",
    "-----END RSA PRIVATE KEY-----",
  ].join("\n"),
  userTextSecret: "api_key=TOPSECRET_USER_TEXT_SECRET_abcdef123456",
  toolArgument: "TOP_SECRET_TOOL_ARGUMENT_PPOISON1",
  jwt: "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.TOPSECRET_JWT_SIG_987654321abc",
} as const;

const NORMALS = {
  prompt: "NORMAL_USER_PROMPT_VISIBLE 帮我总结这段对话",
  memory: "NORMAL_MEMORY_VISIBLE 用户偏好：简洁回复",
  response: "NORMAL_MODEL_RESPONSE_VISIBLE 这是模型的正常回答",
  uuid: "550e8400-e29b-41d4-a716-446655440000",
  fileRef: "file-3f2a91bd",
  prose: "token count was 32 in the last request; model=gpt-5-mini",
} as const;

/* ── 1. inline secret detector：正例 ──────────────────────────────── */

describe("redactTextWithMap 正例（高置信 inline secret 必须替换）", () => {
  const positiveCases: Array<[string, string]> = [
    [`Bearer ${POISONS.bearer}`, "Bearer token"],
    [`Authorization: Bearer ${POISONS.bearer}`, "Authorization bearer"],
    [`Basic dG9wLXNlY3JldC1iYXNpYy1hdXRoOjE=`, "Basic auth"],
    [POISONS.jwt, "JWT"],
    [POISONS.apiKey, "OpenAI-style key"],
    [`anthropic key sk-ant-TOPSECRET-ANTHROPIC-KEY-0123456789abcdef`, "Anthropic key"],
    [`github ghp_TOPSECRETGITHUBTOKEN1234567890`, "GitHub token"],
    [`google AIzaTOPSECRETGOOGLEAPIKEY0123456789abcdef01`, "Google key"],
    [`aws AKIAIOSFODNN7EXAMPLE`, "AWS access key"],
    [POISONS.privateKey, "PEM block"],
    [POISONS.userTextSecret, "kv secret"],
    [`access_token: "TOPSECRET_ACCESS_TOKEN_VALUE_0011223344"`, "kv secret quoted"],
    [`download from /Users/taro/secret/pic.png now`, "inline local path"],
  ];

  for (const [input, label] of positiveCases) {
    it(`替换 ${label}`, () => {
      const result = redactTextWithMap(input);
      // 毒丸片段本身不残留（PEM 的 BEGIN/END 行、Bearer 值等）。
      expect(result.text).not.toContain("TOPSECRET");
      expect(result.replacements.length).toBeGreaterThan(0);
      for (const replacement of result.replacements) {
        expect(Number.isInteger(replacement.start)).toBe(true);
        expect(replacement.end).toBeGreaterThan(replacement.start);
      }
    });
  }

  it("PEM block 整块替换（不是只隐藏第一行，§三十七）", () => {
    const result = redactTextWithMap(`前置说明\n${POISONS.privateKey}\n后置说明`);
    expect(result.text).not.toContain("BEGIN");
    expect(result.text).not.toContain("END");
    expect(result.text).toContain("前置说明");
    expect(result.text).toContain("后置说明");
  });

  it("Bearer 只替换 token 部分、保留字面量（可读性）", () => {
    const result = redactTextWithMap(`Bearer ${POISONS.bearer}`);
    expect(result.text).toContain("Bearer");
    expect(result.text).toContain(INLINE_SECRET_PLACEHOLDER);
  });
});

/* ── 2. inline secret detector：反例（§三十六保守性）──────────────── */

describe("redactTextWithMap 反例（普通文本必须存活，§一百二十九）", () => {
  const negativeCases: Array<[string, string]> = [
    [NORMALS.prompt, "普通用户 prompt"],
    [NORMALS.memory, "记忆文本"],
    [NORMALS.response, "模型回复"],
    [NORMALS.uuid, "UUID"],
    [`参考 file ${NORMALS.fileRef} 处理`, "文件 id"],
    [NORMALS.prose, "研究文本（token/model 词）"],
    [`sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`, "代码 hash"],
    [`任务 id=task-99887766 完成`, "id= 短值"],
    [`日本語のプロンプトもそのまま visible であるべき`, "多语言"],
  ];

  for (const [input, label] of negativeCases) {
    it(`保留 ${label}`, () => {
      const result = redactTextWithMap(input);
      expect(result.text).toBe(input);
      expect(result.replacements).toHaveLength(0);
    });
  }
});

/* ── 3. credential 键 + protocol 路径 ─────────────────────────────── */

describe("credential 键与协议专项路径", () => {
  it("归一化键匹配（x-api-key / X-Api-Key / api_key …）", () => {
    for (const key of ["Authorization", "authorization", "x-api-key", "X-Api-Key", "api_key", "apiKey", "Set-Cookie", "cookie", "x-goog-api-key", "Proxy-Authorization", "client_secret", "refresh_token"]) {
      expect(isCredentialKey(key)).toBe(true);
    }
    for (const key of ["Content-Type", "anthropic-version", "chatgpt-account-id", "model", "messages", "uid", "OpenAI-Beta", "originator"]) {
      expect(isCredentialKey(key)).toBe(false);
    }
  });

  it("header 值替换、结构保留（§二十六/§一百一十四）", () => {
    const headers = {
      "Content-Type": "application/json",
      "anthropic-version": "2023-06-01",
      Authorization: `Bearer ${POISONS.bearer}`,
      "x-api-key": POISONS.apiKey,
    };
    const { value, sanitization } = sanitizeValueForCapture(headers);
    const out = value as Record<string, unknown>;
    expect(out["Content-Type"]).toBe("application/json");
    expect(out["anthropic-version"]).toBe("2023-06-01");
    expect(out.Authorization).toBe("<redacted:credential>");
    expect(out["x-api-key"]).toBe("<redacted:credential>");
    expect(sanitization.redacted).toBe(true);
    expect(JSON.stringify(out)).not.toContain("TOPSECRET");
  });

  it("Volcengine body.user.uid 走协议专项路径（§一百四十九：不依赖 generic key）", () => {
    expect(PROVIDER_BODY_CREDENTIAL_PATHS["volcengine-bigasr-transcription"]).toEqual(["user.uid"]);
    const body = {
      user: { uid: POISONS.volcengineUid },
      audio: { data: "A".repeat(2000) },
      request: { model_name: "bigmodel" },
    };
    const { value, sanitization } = sanitizeValueForCapture(body, {
      secretPaths: secretPathsForProtocol("volcengine-bigasr-transcription"),
    });
    const out = value as any;
    expect(out.user.uid).toBe("<redacted:credential>");
    expect(out.request.model_name).toBe("bigmodel");
    // 裸 base64 音频 externalize
    expect(out.audio.data).toMatchObject({ kind: "external_blob", encoding: "base64" });
    expect(sanitization.redacted).toBe(true);
    expect(JSON.stringify(out)).not.toContain(POISONS.volcengineUid);
  });

  it("无协议时 user.uid 不被 generic denylist 误伤（uid 本身不是 secret 键）", () => {
    const { value } = sanitizeValueForCapture({ user: { uid: "guest-user-1" } });
    expect((value as any).user.uid).toBe("guest-user-1");
  });

  it("嵌套 JSON 中的 credential 键同样替换（§二十四：任意位置）", () => {
    const { value } = sanitizeValueForCapture({
      body: { auth: { access_token: POISONS.oauthRefresh }, content: "normal" },
    });
    const out = value as any;
    expect(out.body.auth.access_token).toBe("<redacted:credential>");
    expect(out.body.content).toBe("normal");
  });
});

/* ── 4. URL sanitization ──────────────────────────────────────────── */

describe("sanitizeCapturedUrl（§二十八/§二十九）", () => {
  it("signed URL → external_reference descriptor（query secret 不保留）", () => {
    const url = `https://media.example.test/output.png?${POISONS.signedUrlSecret}&X-Amz-Expires=3600`;
    const result = sanitizeCapturedUrl(url) as Record<string, unknown>;
    expect(result.kind).toBe("external_reference");
    expect(result.host).toBe("media.example.test");
    expect(result.path).toBe("/output.png");
    expect(result.redacted).toBe(true);
    expect(JSON.stringify(result)).not.toContain("TOPSECRET");
  });

  it("key= query credential 同样转 descriptor", () => {
    const result = sanitizeCapturedUrl("https://api.example.test/v1/generate?key=SECRET_KEY_VALUE_123") as Record<string, unknown>;
    expect(result.kind).toBe("external_reference");
  });

  it("普通 endpoint 原样保留（协议分析价值）", () => {
    const url = "https://api.example.test/v1/messages";
    expect(sanitizeCapturedUrl(url)).toBe(url);
  });

  it("data: URL → external_blob descriptor", () => {
    const result = sanitizeCapturedUrl("data:image/png;base64,iVBORw0KGgo=") as Record<string, unknown>;
    expect(result.kind).toBe("external_blob");
    expect(result.mediaType).toBe("image/png");
  });
});

/* ── 5. 本地路径 ──────────────────────────────────────────────────── */

describe("本地绝对路径（§三十）", () => {
  it("整串路径 → local_file_reference descriptor（保 basename）", () => {
    expect(looksLikeLocalPath("/Users/taro/media/ref.png")).toBe(true);
    const { value, sanitization } = sanitizeValueForCapture("/Users/taro/media/ref.png");
    expect(value).toMatchObject({ kind: "local_file_reference", basename: "ref.png" });
    expect(sanitization.redacted).toBe(true);
    expect(JSON.stringify(value)).not.toContain("/Users/taro");
  });

  it("Windows 路径同样处理", () => {
    expect(looksLikeLocalPath("C:\\Users\\taro\\media\\ref.png")).toBe(true);
  });

  it("普通 URL 字符串不是本地路径", () => {
    expect(looksLikeLocalPath("https://example.test/a.png")).toBe(false);
  });
});

/* ── 6. 二进制 externalization ────────────────────────────────────── */

describe("二进制 externalization（§三十一/§三十二）", () => {
  it("Buffer / Uint8Array / Blob → external_blob，不保存字节", () => {
    const buffer = Buffer.from("topsecret-binary-bytes-should-never-be-captured");
    const { value, sanitization } = sanitizeValueForCapture({ frame: buffer });
    expect((value as any).frame).toMatchObject({ kind: "external_blob", encoding: "binary" });
    expect((value as any).frame.byteLength).toBe(buffer.length);
    expect(sanitization.actions.some((a) => a.action === "externalized")).toBe(true);
    expect(JSON.stringify(value)).not.toContain("topsecret-binary");
  });

  it("data URL 字符串 → external_blob descriptor", () => {
    const { value } = sanitizeValueForCapture({ image: "data:image/png;base64,iVBORw0KGgoAAAANS" });
    expect((value as any).image).toMatchObject({ kind: "external_blob", mediaType: "image/png", encoding: "base64-data-url" });
  });

  it("裸 base64（≥1024、采样通过）→ external_blob；短 base64 保留", () => {
    const long = "qwertyuiopASDFGHJKL0123456789+/".repeat(64); // 2048 chars base64 charset
    const { value } = sanitizeValueForCapture({ a: long, b: "shortABC123==" });
    expect((value as any).a).toMatchObject({ kind: "external_blob", encoding: "base64" });
    expect((value as any).b).toBe("shortABC123==");
  });

  it("FormData：文本字段保留、Blob externalize（§一百四十八）", () => {
    const form = new FormData();
    form.set("model", "whisper-1");
    form.set("language", "zh");
    form.set("file", new Blob([Buffer.from("audio-bytes-secret")], { type: "audio/wav" }), "audio.wav");
    const { value, sanitization } = sanitizeValueForCapture(form);
    const out = value as any;
    expect(out.kind).toBe("multipart_form_data");
    expect(out.fields.model).toBe("whisper-1");
    expect(out.fields.language).toBe("zh");
    expect(out.files).toHaveLength(1);
    expect(out.files[0]).toMatchObject({ field: "file", kind: "external_blob", mediaType: "audio/wav" });
    expect(JSON.stringify(value)).not.toContain("audio-bytes-secret");
    expect(sanitization.actions.some((a) => a.reason === "form-file")).toBe(true);
  });

  it("AbortSignal（google payload 内嵌）按 unsupported 剔除不 throw", () => {
    const { value, sanitization } = sanitizeValueForCapture({ contents: [], config: { abortSignal: AbortSignal.timeout(1000) } });
    expect((value as any).config.abortSignal).toBeNull();
    expect(sanitization.degraded).toBe(true);
  });
});

/* ── 7. 原对象不可变（§十五/§一百三十）────────────────────────────── */

describe("原对象不可变（copy-on-capture）", () => {
  it("capture 前后原对象业务语义完全一致", () => {
    const original = {
      headers: { Authorization: `Bearer ${POISONS.bearer}`, "Content-Type": "application/json" },
      body: {
        model: "gpt-test",
        messages: [
          { role: "system", content: `persona text ${POISONS.apiKey}` },
          { role: "user", content: NORMALS.prompt },
        ],
        nested: { list: [1, 2, { deep: { secret: "TOPSECRET-NESTED-SECRET-9a8b7c6d" } }] },
      },
    };
    const snapshot = JSON.stringify(original);
    const result = sanitizeValueForCapture(original);
    expect(JSON.stringify(original)).toBe(snapshot);
    expect(result.value).not.toBe(original);
    expect((result.value as any).body).not.toBe(original.body);
    expect(JSON.stringify(result.value)).not.toContain("TOPSECRET");
    expect((result.value as any).body.messages[1].content).toBe(NORMALS.prompt);
  });
});

/* ── 8. 资源上限（§一百三十二）────────────────────────────────────── */

describe("资源上限（显式 truncated，不 OOM）", () => {
  it("超长字符串截断并标注", () => {
    const huge = "text chunk ".repeat(Math.ceil((MODEL_CALL_PAYLOAD_CAPTURE_LIMITS.maxStringChars + 5000) / 11));
    const { value, sanitization } = sanitizeValueForCapture(huge);
    expect((value as string).length).toBe(MODEL_CALL_PAYLOAD_CAPTURE_LIMITS.maxStringChars);
    expect(sanitization.truncated).toBe(true);
  });

  it("超长数组折叠并标注", () => {
    const array = Array.from({ length: MODEL_CALL_PAYLOAD_CAPTURE_LIMITS.maxArrayItems + 100 }, (_, i) => i);
    const { value, sanitization } = sanitizeValueForCapture(array);
    expect((value as unknown[]).length).toBe(MODEL_CALL_PAYLOAD_CAPTURE_LIMITS.maxArrayItems);
    expect(sanitization.truncated).toBe(true);
  });

  it("record 预算耗尽后停止复制（degraded）", () => {
    const many = Array.from({ length: 64 }, () => ({ text: "payload text ".repeat(Math.ceil(MODEL_CALL_PAYLOAD_CAPTURE_LIMITS.maxStringChars / 13)) }));
    const { value, sanitization } = sanitizeValueForCapture(many);
    expect(sanitization.truncated).toBe(true);
    const items = value as unknown[];
    expect(items.length).toBeLessThan(64);
    expect(items.at(-1)).toBeNull();
  });
});

/* ── 9. 循环引用（§一百三十一）────────────────────────────────────── */

describe("循环引用", () => {
  it("obj.self = obj 不 stack overflow，标记 unsupported", () => {
    const obj: Record<string, unknown> = { name: "cycle-root" };
    obj.self = obj;
    const { value, sanitization } = sanitizeValueForCapture(obj);
    expect((value as any).name).toBe("cycle-root");
    expect((value as any).self).toBeNull();
    expect(sanitization.degraded).toBe(true);
    expect(sanitization.actions.some((a) => a.reason === "cyclic-reference")).toBe(true);
  });

  it("共享引用（非环）保持为两份独立拷贝（detached 语义）", () => {
    const shared = { leaf: true };
    const root = { a: shared, b: shared };
    const { value } = sanitizeValueForCapture(root);
    const out = value as any;
    expect(out.a).toEqual({ leaf: true });
    expect(out.b).toEqual({ leaf: true });
  });
});

/* ── 10. span remap（§四十八～§五十）─────────────────────────────── */

describe("redaction offset mapping / span remap", () => {
  it("无重叠 span 平移 delta；重叠 span 降级 null", () => {
    const original = `前缀说明 token=TOPSECRET_SPAN_SECRET_abcdef12 后缀说明`;
    const result = redactTextWithMap(original);
    // 前缀 span（[0, 5)）不重叠 → 平移后仍指向同一文本
    const before = remapSpanAfterRedaction({ start: 0, end: 5 }, result.replacements);
    expect(before.degraded).toBe(false);
    expect(result.text.slice(before.span!.start, before.span!.end)).toBe(original.slice(0, 5));
    // 重叠 span（覆盖 secret）→ null + degraded
    const secretStart = original.indexOf("TOPSECRET");
    const overlap = remapSpanAfterRedaction(
      { start: secretStart - 2, end: secretStart + 10 },
      result.replacements,
    );
    expect(overlap.degraded).toBe(true);
    expect(overlap.span).toBeNull();
    // 后缀 span 平移正确
    const tailStart = original.indexOf("后缀");
    const tail = remapSpanAfterRedaction({ start: tailStart, end: tailStart + 2 }, result.replacements);
    expect(tail.degraded).toBe(false);
    expect(result.text.slice(tail.span!.start, tail.span!.end)).toBe("后缀");
  });

  it("多个 replacement 的累计 delta 正确", () => {
    const original = "a token=SECRETVALUE12345678 b api_key=OTHERSECRET998877665544 c";
    const result = redactTextWithMap(original);
    const cAt = original.indexOf("c");
    const tail = remapSpanAfterRedaction({ start: cAt, end: cAt + 1 }, result.replacements);
    expect(result.text.slice(tail.span!.start, tail.span!.end)).toBe("c");
  });
});
