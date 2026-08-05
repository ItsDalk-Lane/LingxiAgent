import { beforeEach, describe, expect, it } from "vitest";

import { handleRequest } from "../tools/alpha-gate-worker/core.mjs";

/**
 * 内存版 KV：只实现 Worker 端真正用到的 get / put / delete / list 四个方法，
 * 语义对齐 Cloudflare KV（get 缺键返回 null，list 按前缀分页）。
 */
function createMemoryKv() {
  const store = new Map<string, string>();
  return {
    store,
    async get(key: string) {
      return store.has(key) ? store.get(key) : null;
    },
    async put(key: string, value: string) {
      store.set(key, String(value));
    },
    async delete(key: string) {
      store.delete(key);
    },
    async list(options: { prefix?: string; cursor?: string; limit?: number } = {}) {
      const prefix = options.prefix || "";
      const limit = options.limit || 1000;
      const names = [...store.keys()].filter((key) => key.startsWith(prefix)).sort();
      const start = options.cursor ? Number(options.cursor) : 0;
      const page = names.slice(start, start + limit);
      const nextIndex = start + page.length;
      const complete = nextIndex >= names.length;
      return {
        keys: page.map((name) => ({ name })),
        list_complete: complete,
        cursor: complete ? undefined : String(nextIndex),
      };
    },
  };
}

const ADMIN_TOKEN = "owner-token-for-tests";
const FEED_URL = "https://feed.example/alpha/";

function createHarness(envOverrides: Record<string, string> = {}) {
  const kv = createMemoryKv();
  const env = { FEED_URL, ADMIN_TOKEN, ...envOverrides };
  let clock = Date.parse("2026-08-01T00:00:00.000Z");
  // 确定性 RNG：xorshift32 取高位字节。低位循环极短的 LCG 会造出重复码，
  // 那是随机源的毛病，不是被测逻辑的毛病。
  let seed = 0x9e3779b9;
  const randomBytes = (length: number) => {
    const out = new Uint8Array(length);
    for (let i = 0; i < length; i += 1) {
      seed ^= seed << 13;
      seed ^= seed >>> 17;
      seed ^= seed << 5;
      seed >>>= 0;
      out[i] = (seed >>> 24) & 0xff;
    }
    return out;
  };
  const deps = { kv, env, now: () => clock, randomBytes };

  const call = (request: Request) => handleRequest(request, deps);

  return {
    kv,
    env,
    deps,
    advanceMs(ms: number) {
      clock += ms;
    },
    call,
    post(path: string, body: unknown, headers: Record<string, string> = {}) {
      return call(
        new Request(`https://gate.example${path}`, {
          method: "POST",
          headers: {
            "content-type": "application/json",
            "CF-Connecting-IP": "203.0.113.9",
            ...headers,
          },
          body: typeof body === "string" ? body : JSON.stringify(body),
        }),
      );
    },
    get(path: string, headers: Record<string, string> = {}) {
      return call(
        new Request(`https://gate.example${path}`, {
          method: "GET",
          headers: { "CF-Connecting-IP": "203.0.113.9", ...headers },
        }),
      );
    },
    adminHeaders() {
      return { authorization: `Bearer ${ADMIN_TOKEN}` };
    },
    readCode(code: string) {
      const raw = kv.store.get(`code:${code}`);
      return raw ? JSON.parse(raw) : null;
    },
    readTotal() {
      const raw = kv.store.get("meta:total");
      return raw === undefined ? 0 : Number(raw);
    },
  };
}

const DEVICE_A = "a".repeat(64);
const DEVICE_B = "b".repeat(64);

async function mintRoot(harness: ReturnType<typeof createHarness>, count = 1, generation = 0) {
  const response = await harness.post("/admin/mint", { count, generation }, harness.adminHeaders());
  const payload = await response.json();
  expect(response.status).toBe(200);
  expect(payload.ok).toBe(true);
  return payload.codes as string[];
}

describe("alpha gate worker: POST /redeem", () => {
  let harness: ReturnType<typeof createHarness>;

  beforeEach(() => {
    harness = createHarness();
  });

  it("redeems an unused code, binds the device, and mints the fission children", async () => {
    const [root] = await mintRoot(harness);
    expect(root).toMatch(/^HANA-[23456789ABCDEFGHJKLMNPQRSTUVWXYZ]{4}-[23456789ABCDEFGHJKLMNPQRSTUVWXYZ]{4}-[23456789ABCDEFGHJKLMNPQRSTUVWXYZ]{4}$/);

    const response = await harness.post("/redeem", { code: root, deviceIdHash: DEVICE_A });
    const payload = await response.json();

    expect(response.status).toBe(200);
    expect(payload.ok).toBe(true);
    expect(payload.feedUrl).toBe(FEED_URL);
    expect(payload.childCodes).toHaveLength(2);

    const parent = harness.readCode(root);
    expect(parent.status).toBe("redeemed");
    expect(parent.deviceIdHash).toBe(DEVICE_A);
    expect(parent.redeemedAt).toBe("2026-08-01T00:00:00.000Z");
    expect(parent.children).toEqual(payload.childCodes);

    for (const child of payload.childCodes) {
      const record = harness.readCode(child);
      expect(record).toMatchObject({ status: "unused", parent: root, generation: 1, children: [] });
    }

    // 1 root + 2 children
    expect(harness.readTotal()).toBe(3);
  });

  it("is idempotent for the same device and mints no extra codes", async () => {
    const [root] = await mintRoot(harness);
    const first = await (await harness.post("/redeem", { code: root, deviceIdHash: DEVICE_A })).json();
    const totalAfterFirst = harness.readTotal();

    const second = await harness.post("/redeem", { code: root, deviceIdHash: DEVICE_A });
    const payload = await second.json();

    expect(second.status).toBe(200);
    expect(payload.ok).toBe(true);
    expect(payload.feedUrl).toBe(FEED_URL);
    expect(payload.childCodes).toEqual(first.childCodes);
    expect(harness.readTotal()).toBe(totalAfterFirst);
  });

  it("accepts the code in lower case and with surrounding whitespace", async () => {
    const [root] = await mintRoot(harness);
    const response = await harness.post("/redeem", { code: `  ${root.toLowerCase()}  `, deviceIdHash: DEVICE_A });
    const payload = await response.json();
    expect(response.status).toBe(200);
    expect(payload.ok).toBe(true);
  });

  it("answers unknown, revoked, and other-device codes with one indistinguishable rejection", async () => {
    const [redeemed] = await mintRoot(harness);
    await harness.post("/redeem", { code: redeemed, deviceIdHash: DEVICE_A });

    const [revoked] = await mintRoot(harness);
    await harness.post("/admin/revoke", { code: revoked }, harness.adminHeaders());

    const otherDevice = await harness.post("/redeem", { code: redeemed, deviceIdHash: DEVICE_B });
    const revokedResponse = await harness.post("/redeem", { code: revoked, deviceIdHash: DEVICE_B });
    const unknownResponse = await harness.post("/redeem", { code: "HANA-2345-6789-ABCD", deviceIdHash: DEVICE_B });
    const malformedResponse = await harness.post("/redeem", { code: "NOT-A-CODE", deviceIdHash: DEVICE_B });

    const bodies = await Promise.all([
      otherDevice.json(),
      revokedResponse.json(),
      unknownResponse.json(),
      malformedResponse.json(),
    ]);

    const statuses = [otherDevice.status, revokedResponse.status, unknownResponse.status, malformedResponse.status];
    expect(new Set(statuses)).toEqual(new Set([404]));
    for (const body of bodies) {
      expect(body).toEqual(bodies[0]);
      expect(body.ok).toBe(false);
      expect(typeof body.error).toBe("string");
      expect(body.error.length).toBeGreaterThan(0);
    }
    // 拒绝响应不得泄露任何谱系细节
    for (const body of bodies) {
      expect(Object.keys(body).sort()).toEqual(["error", "ok"]);
    }
  });

  it("mints exactly FISSION_COUNT children", async () => {
    const wide = createHarness({ FISSION_COUNT: "5" });
    const [root] = await mintRoot(wide);
    const payload = await (await wide.post("/redeem", { code: root, deviceIdHash: DEVICE_A })).json();
    expect(payload.childCodes).toHaveLength(5);
    expect(wide.readTotal()).toBe(6);
  });

  it("stops the fission at MAX_GENERATION and still redeems successfully", async () => {
    const shallow = createHarness({ MAX_GENERATION: "1" });
    const [root] = await mintRoot(shallow);
    const first = await (await shallow.post("/redeem", { code: root, deviceIdHash: DEVICE_A })).json();
    expect(first.childCodes).toHaveLength(2);

    const child = first.childCodes[0];
    const response = await shallow.post("/redeem", { code: child, deviceIdHash: DEVICE_B });
    const payload = await response.json();

    expect(response.status).toBe(200);
    expect(payload.ok).toBe(true);
    expect(payload.feedUrl).toBe(FEED_URL);
    expect(payload.childCodes).toEqual([]);
    expect(shallow.readCode(child).status).toBe("redeemed");
  });

  it("cuts the fission down to the remaining global capacity", async () => {
    const capped = createHarness({ GLOBAL_CAP: "2" });
    const [root] = await mintRoot(capped);
    const payload = await (await capped.post("/redeem", { code: root, deviceIdHash: DEVICE_A })).json();

    expect(payload.ok).toBe(true);
    expect(payload.childCodes).toHaveLength(1);
    expect(capped.readTotal()).toBe(2);

    const child = payload.childCodes[0];
    const second = await (await capped.post("/redeem", { code: child, deviceIdHash: DEVICE_B })).json();
    expect(second.ok).toBe(true);
    expect(second.childCodes).toEqual([]);
    expect(capped.readTotal()).toBe(2);
  });

  it("rejects malformed JSON and missing fields with a structured 400", async () => {
    const badJson = await harness.post("/redeem", "{ not json");
    const badJsonBody = await badJson.json();
    expect(badJson.status).toBe(400);
    expect(badJsonBody.ok).toBe(false);
    expect(typeof badJsonBody.error).toBe("string");

    const arrayBody = await harness.post("/redeem", [1, 2, 3]);
    expect(arrayBody.status).toBe(400);

    const missing = await harness.post("/redeem", { code: "HANA-2345-6789-ABCD" });
    expect(missing.status).toBe(400);
    expect((await missing.json()).ok).toBe(false);

    const badHash = await harness.post("/redeem", { code: "HANA-2345-6789-ABCD", deviceIdHash: "nope" });
    expect(badHash.status).toBe(400);
  });

  it("reports a missing feed address instead of redeeming the code", async () => {
    const unconfigured = createHarness({ FEED_URL: "" });
    const [root] = await mintRoot(unconfigured);
    const response = await unconfigured.post("/redeem", { code: root, deviceIdHash: DEVICE_A });
    const payload = await response.json();

    expect(response.status).toBe(500);
    expect(payload.ok).toBe(false);
    expect(unconfigured.readCode(root).status).toBe("unused");
  });

  it("throttles per IP once RATE_LIMIT_PER_HOUR is exhausted and recovers after the window", async () => {
    const throttled = createHarness({ RATE_LIMIT_PER_HOUR: "2" });
    const [root] = await mintRoot(throttled);

    const first = await throttled.post("/redeem", { code: root, deviceIdHash: DEVICE_A });
    expect(first.status).toBe(200);
    const second = await throttled.post("/redeem", { code: root, deviceIdHash: DEVICE_A });
    expect(second.status).toBe(200);

    const third = await throttled.post("/redeem", { code: root, deviceIdHash: DEVICE_A });
    const thirdBody = await third.json();
    expect(third.status).toBe(429);
    expect(thirdBody.ok).toBe(false);
    expect(typeof thirdBody.error).toBe("string");

    // 另一个 IP 不受影响
    const otherIp = await throttled.post(
      "/redeem",
      { code: root, deviceIdHash: DEVICE_A },
      { "CF-Connecting-IP": "198.51.100.4" },
    );
    expect(otherIp.status).toBe(200);

    throttled.advanceMs(3600 * 1000 + 1);
    const afterWindow = await throttled.post("/redeem", { code: root, deviceIdHash: DEVICE_A });
    expect(afterWindow.status).toBe(200);
  });
});

describe("alpha gate worker: admin endpoints", () => {
  let harness: ReturnType<typeof createHarness>;

  beforeEach(() => {
    harness = createHarness();
  });

  it("requires a bearer token on mint, revoke, and tree", async () => {
    const anonymous = [
      await harness.post("/admin/mint", { count: 1 }),
      await harness.post("/admin/revoke", { code: "HANA-2345-6789-ABCD" }),
      await harness.get("/admin/tree"),
    ];
    for (const response of anonymous) {
      expect(response.status).toBe(401);
      expect((await response.json()).ok).toBe(false);
    }

    const wrongToken = { authorization: "Bearer not-the-owner-token" };
    const rejected = [
      await harness.post("/admin/mint", { count: 1 }, wrongToken),
      await harness.post("/admin/revoke", { code: "HANA-2345-6789-ABCD" }, wrongToken),
      await harness.get("/admin/tree", wrongToken),
    ];
    for (const response of rejected) {
      expect(response.status).toBe(401);
    }

    expect((await harness.post("/admin/mint", { count: 1 }, harness.adminHeaders())).status).toBe(200);
    expect((await harness.get("/admin/tree", harness.adminHeaders())).status).toBe(200);
  });

  it("refuses admin access when no ADMIN_TOKEN is configured", async () => {
    const open = createHarness({ ADMIN_TOKEN: "" });
    const missingHeader = await open.get("/admin/tree");
    const emptyBearer = await open.get("/admin/tree", { authorization: "Bearer " });
    expect(missingHeader.status).toBe(401);
    expect(emptyBearer.status).toBe(401);
  });

  it("mints the requested batch of root codes and tracks the total", async () => {
    const codes = await mintRoot(harness, 3);
    expect(codes).toHaveLength(3);
    expect(new Set(codes).size).toBe(3);
    expect(harness.readTotal()).toBe(3);
    for (const code of codes) {
      expect(harness.readCode(code)).toMatchObject({ status: "unused", parent: null, generation: 0, children: [] });
    }
  });

  it("refuses a mint batch that would break the global cap", async () => {
    const capped = createHarness({ GLOBAL_CAP: "2" });
    const response = await capped.post("/admin/mint", { count: 3 }, capped.adminHeaders());
    const payload = await response.json();
    expect(response.status).toBe(409);
    expect(payload.ok).toBe(false);
    expect(capped.readTotal()).toBe(0);
  });

  it("rejects an out-of-range mint count", async () => {
    const zero = await harness.post("/admin/mint", { count: 0 }, harness.adminHeaders());
    const negative = await harness.post("/admin/mint", { count: -1 }, harness.adminHeaders());
    const fractional = await harness.post("/admin/mint", { count: 1.5 }, harness.adminHeaders());
    for (const response of [zero, negative, fractional]) {
      expect(response.status).toBe(400);
    }
  });

  it("revokes an entire subtree when cascading", async () => {
    const [root] = await mintRoot(harness);
    const firstGeneration = (await (await harness.post("/redeem", { code: root, deviceIdHash: DEVICE_A })).json()).childCodes;
    const secondGeneration = (
      await (await harness.post("/redeem", { code: firstGeneration[0], deviceIdHash: DEVICE_B })).json()
    ).childCodes;
    expect(secondGeneration).toHaveLength(2);

    const response = await harness.post("/admin/revoke", { code: root }, harness.adminHeaders());
    const payload = await response.json();

    expect(response.status).toBe(200);
    expect(payload.ok).toBe(true);
    const expected = [root, ...firstGeneration, ...secondGeneration].sort();
    expect([...payload.revoked].sort()).toEqual(expected);
    for (const code of expected) {
      expect(harness.readCode(code).status).toBe("revoked");
    }
  });

  it("revokes only the named code when cascade is false", async () => {
    const [root] = await mintRoot(harness);
    const children = (await (await harness.post("/redeem", { code: root, deviceIdHash: DEVICE_A })).json()).childCodes;

    const response = await harness.post("/admin/revoke", { code: root, cascade: false }, harness.adminHeaders());
    const payload = await response.json();

    expect(response.status).toBe(200);
    expect(payload.revoked).toEqual([root]);
    for (const child of children) {
      expect(harness.readCode(child).status).toBe("unused");
    }
  });

  it("reports an unknown code honestly to the authenticated owner", async () => {
    const response = await harness.post("/admin/revoke", { code: "HANA-2345-6789-ABCD" }, harness.adminHeaders());
    expect(response.status).toBe(404);
    expect((await response.json()).ok).toBe(false);
  });

  it("returns the whole genealogy from /admin/tree", async () => {
    const [root] = await mintRoot(harness);
    const children = (await (await harness.post("/redeem", { code: root, deviceIdHash: DEVICE_A })).json()).childCodes;

    const response = await harness.get("/admin/tree", harness.adminHeaders());
    const payload = await response.json();

    expect(response.status).toBe(200);
    expect(payload.ok).toBe(true);
    expect(payload.total).toBe(3);
    expect(payload.count).toBe(3);
    expect(payload.roots).toEqual([root]);
    expect(Object.keys(payload.codes).sort()).toEqual([root, ...children].sort());
    expect(payload.codes[root].children).toEqual(children);
    expect(payload.codes[children[0]].parent).toBe(root);
  });
});

describe("alpha gate worker: routing and configuration", () => {
  it("answers unknown paths with 404 and wrong methods with 405", async () => {
    const harness = createHarness();
    const unknown = await harness.get("/nope");
    expect(unknown.status).toBe(404);

    const wrongMethod = await harness.get("/redeem");
    expect(wrongMethod.status).toBe(405);
    expect(wrongMethod.headers.get("allow")).toBe("POST");
  });

  it("fails loudly on a malformed numeric setting instead of falling back", async () => {
    const broken = createHarness({ FISSION_COUNT: "many" });
    const response = await broken.post("/redeem", { code: "HANA-2345-6789-ABCD", deviceIdHash: DEVICE_A });
    const payload = await response.json();
    expect(response.status).toBe(500);
    expect(payload.ok).toBe(false);
    expect(payload.error).toContain("FISSION_COUNT");
  });

  it("fails loudly when the KV binding is missing", async () => {
    const response = await handleRequest(
      new Request("https://gate.example/redeem", { method: "POST", body: "{}" }),
      { env: { FEED_URL, ADMIN_TOKEN } },
    );
    expect(response.status).toBe(500);
    expect((await response.json()).ok).toBe(false);
  });

  it("fails loudly when the minted-code counter is corrupted", async () => {
    const harness = createHarness();
    await mintRoot(harness);
    harness.kv.store.set("meta:total", "not-a-number");
    const [root] = [...harness.kv.store.keys()].filter((key) => key.startsWith("code:")).map((key) => key.slice(5));
    const response = await harness.post("/redeem", { code: root, deviceIdHash: DEVICE_A });
    expect(response.status).toBe(500);
    expect((await response.json()).ok).toBe(false);
  });
});
