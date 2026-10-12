import dns from "node:dns";
import http from "node:http";
import https from "node:https";
import net from "node:net";

import { isKnowledgeError, KnowledgeError } from "./errors.ts";

export const DEFAULT_WEB_SNAPSHOT_MAX_BYTES = 10 * 1024 * 1024;
const FETCH_TIMEOUT_MS = 20_000;
const MAX_REDIRECTS = 5;

const blockedAddresses = new net.BlockList();
for (const [address, prefix] of [
  ["0.0.0.0", 8],
  ["10.0.0.0", 8],
  ["100.64.0.0", 10],
  ["127.0.0.0", 8],
  ["169.254.0.0", 16],
  ["172.16.0.0", 12],
  ["192.0.0.0", 24],
  ["192.0.2.0", 24],
  ["192.168.0.0", 16],
  ["198.18.0.0", 15],
  ["198.51.100.0", 24],
  ["203.0.113.0", 24],
  ["224.0.0.0", 4],
  ["240.0.0.0", 4],
] as const) blockedAddresses.addSubnet(address, prefix, "ipv4");
for (const [address, prefix] of [
  ["::", 128],
  ["::1", 128],
  ["64:ff9b::", 96],
  ["64:ff9b:1::", 48],
  ["100::", 64],
  ["2001::", 32],
  ["2001:2::", 48],
  ["2001:db8::", 32],
  ["2001:10::", 28],
  ["2001:20::", 28],
  ["2002::", 16],
  ["fc00::", 7],
  ["fe80::", 10],
  ["ff00::", 8],
] as const) blockedAddresses.addSubnet(address, prefix, "ipv6");

interface ResolvedAddress {
  address: string;
  family: 4 | 6;
}

interface SnapshotResponse {
  status: number;
  headers: http.IncomingHttpHeaders;
  bytes: Buffer;
}

export interface WebSnapshotFetchResult {
  originalUrl: string;
  finalUrl: string;
  mimeType: "text/html";
  bytes: Buffer;
  fetchedAt: string;
}

export interface WebSnapshotFetchOptions {
  maxBytes?: number;
  now?: () => string;
  resolveHost?: (hostname: string) => Promise<ResolvedAddress[]>;
  requestOnce?: (url: URL, address: ResolvedAddress, maxBytes: number) => Promise<SnapshotResponse>;
}

function normalizedUrl(value: unknown): URL {
  if (typeof value !== "string" || !value.trim() || value.length > 2048) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Web source URL is invalid");
  }
  let url: URL;
  try {
    url = new URL(value.trim());
  } catch {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Web source URL is invalid");
  }
  if (
    (url.protocol !== "http:" && url.protocol !== "https:")
    || url.username
    || url.password
    || !url.hostname
    || (url.port && !(
      (url.protocol === "http:" && url.port === "80")
      || (url.protocol === "https:" && url.port === "443")
    ))
  ) {
    throw new KnowledgeError("KNOWLEDGE_WEB_URL_BLOCKED", "Web source URL is not allowed");
  }
  url.hash = "";
  return url;
}

function addressIsBlocked(entry: ResolvedAddress): boolean {
  if (net.isIP(entry.address) !== entry.family) return true;
  return blockedAddresses.check(entry.address, entry.family === 4 ? "ipv4" : "ipv6");
}

function dnsHostname(url: URL): string {
  return url.hostname.startsWith("[") && url.hostname.endsWith("]")
    ? url.hostname.slice(1, -1)
    : url.hostname;
}

async function defaultResolveHost(hostname: string): Promise<ResolvedAddress[]> {
  if (net.isIP(hostname)) {
    return [{ address: hostname, family: net.isIP(hostname) as 4 | 6 }];
  }
  try {
    return await dns.promises.lookup(hostname, { all: true, verbatim: true }) as ResolvedAddress[];
  } catch {
    throw new KnowledgeError("KNOWLEDGE_WEB_FETCH_FAILED", "Web source hostname could not be resolved");
  }
}

function defaultRequestOnce(url: URL, resolved: ResolvedAddress, maxBytes: number): Promise<SnapshotResponse> {
  return new Promise((resolve, reject) => {
    const transport = url.protocol === "https:" ? https : http;
    const requestOptions: https.RequestOptions = {
      method: "GET",
      headers: {
        Accept: "text/html,application/xhtml+xml",
        "Accept-Encoding": "identity",
        "User-Agent": "LingxiAgent-KnowledgeSnapshot/1.0",
      },
      lookup: ((_hostname: string, options: any, callback: any) => {
        if (options?.all) callback(null, [{ address: resolved.address, family: resolved.family }]);
        else callback(null, resolved.address, resolved.family);
      }) as any,
      ...(url.protocol === "https:" && net.isIP(dnsHostname(url)) === 0
        ? { servername: dnsHostname(url) }
        : {}),
    };
    let settled = false;
    const finishReject = (error: unknown) => {
      if (settled) return;
      settled = true;
      clearTimeout(deadline);
      reject(error);
    };
    const finishResolve = (response: SnapshotResponse) => {
      if (settled) return;
      settled = true;
      clearTimeout(deadline);
      resolve(response);
    };
    const request = transport.request(url, requestOptions, response => {
      const contentLength = Number(response.headers["content-length"] || 0);
      if (Number.isFinite(contentLength) && contentLength > maxBytes) {
        response.destroy();
        finishReject(new KnowledgeError("KNOWLEDGE_WEB_TOO_LARGE", "Web source response is too large"));
        return;
      }
      const chunks: Buffer[] = [];
      let total = 0;
      response.on("data", (chunk: Buffer | string) => {
        const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
        total += bytes.length;
        if (total > maxBytes) {
          response.destroy(new KnowledgeError("KNOWLEDGE_WEB_TOO_LARGE", "Web source response is too large"));
          return;
        }
        chunks.push(bytes);
      });
      response.on("end", () => finishResolve({
        status: response.statusCode || 0,
        headers: response.headers,
        bytes: Buffer.concat(chunks),
      }));
      response.on("error", finishReject);
    });
    const deadline = setTimeout(() => {
      request.destroy(new KnowledgeError("KNOWLEDGE_WEB_FETCH_FAILED", "Web source request timed out"));
    }, FETCH_TIMEOUT_MS);
    request.setTimeout(FETCH_TIMEOUT_MS, () => {
      request.destroy(new KnowledgeError("KNOWLEDGE_WEB_FETCH_FAILED", "Web source request timed out"));
    });
    request.on("error", finishReject);
    request.end();
  });
}

export async function fetchCitationGradeWebSnapshot(
  value: unknown,
  options: WebSnapshotFetchOptions = {},
): Promise<WebSnapshotFetchResult> {
  const maxBytes = options.maxBytes ?? DEFAULT_WEB_SNAPSHOT_MAX_BYTES;
  if (!Number.isSafeInteger(maxBytes) || maxBytes <= 0) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Web source size limit is invalid");
  }
  const original = normalizedUrl(value);
  let current = original;
  const resolveHost = options.resolveHost || defaultResolveHost;
  const requestOnce = options.requestOnce || defaultRequestOnce;

  for (let hop = 0; hop <= MAX_REDIRECTS; hop += 1) {
    let addresses: ResolvedAddress[];
    try {
      addresses = await resolveHost(dnsHostname(current));
    } catch (error) {
      if (isKnowledgeError(error)) throw error;
      throw new KnowledgeError("KNOWLEDGE_WEB_FETCH_FAILED", "Web source hostname could not be resolved");
    }
    if (addresses.length === 0 || addresses.some(addressIsBlocked)) {
      throw new KnowledgeError("KNOWLEDGE_WEB_URL_BLOCKED", "Web source resolved to a blocked network");
    }
    let response: SnapshotResponse;
    try {
      response = await requestOnce(current, addresses[0], maxBytes);
    } catch (error) {
      if (isKnowledgeError(error)) throw error;
      throw new KnowledgeError("KNOWLEDGE_WEB_FETCH_FAILED", "Web source request failed");
    }
    if ([301, 302, 303, 307, 308].includes(response.status)) {
      const location = Array.isArray(response.headers.location)
        ? response.headers.location[0]
        : response.headers.location;
      if (!location || hop === MAX_REDIRECTS) {
        throw new KnowledgeError("KNOWLEDGE_WEB_FETCH_FAILED", "Web source redirect limit was exceeded");
      }
      try {
        current = normalizedUrl(new URL(location, current).href);
      } catch (error) {
        if (isKnowledgeError(error)) throw error;
        throw new KnowledgeError("KNOWLEDGE_WEB_FETCH_FAILED", "Web source returned an invalid redirect");
      }
      continue;
    }
    if (response.status < 200 || response.status >= 300) {
      throw new KnowledgeError("KNOWLEDGE_WEB_FETCH_FAILED", "Web source returned an unsuccessful response");
    }
    const contentEncoding = String(response.headers["content-encoding"] || "identity").toLowerCase();
    if (contentEncoding !== "identity") {
      throw new KnowledgeError("KNOWLEDGE_WEB_TYPE_UNSUPPORTED", "Compressed web snapshots are not accepted");
    }
    const contentType = String(response.headers["content-type"] || "").toLowerCase();
    if (!/^text\/html(?:\s*;|$)/u.test(contentType)) {
      throw new KnowledgeError("KNOWLEDGE_WEB_TYPE_UNSUPPORTED", "Web source must return HTML");
    }
    const declaredLength = Number(response.headers["content-length"] || 0);
    if (
      !Buffer.isBuffer(response.bytes)
      || response.bytes.length === 0
      || response.bytes.length > maxBytes
      || (Number.isFinite(declaredLength) && declaredLength > maxBytes)
    ) {
      throw new KnowledgeError("KNOWLEDGE_WEB_TOO_LARGE", "Web source response size is invalid");
    }
    return {
      originalUrl: original.href,
      finalUrl: current.href,
      mimeType: "text/html",
      bytes: response.bytes,
      fetchedAt: (options.now || (() => new Date().toISOString()))(),
    };
  }
  throw new KnowledgeError("KNOWLEDGE_WEB_FETCH_FAILED", "Web source could not be fetched");
}
