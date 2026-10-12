# R02 FINAL STAGE REVIEW (independent reviewer R1, first run)

Reviewer: R02-FINAL-STAGE-REVIEWER-R1 (fresh identity; no involvement in any implementation, repair,
or prior review round). Review window: 2026-09-28T20:40–21:00 UTC. All reviewer rerun artifacts in
`/tmp/r02-reviewer-r1/` only; no repository file was modified (newest tracked-change mtime
20:37:25Z predates this session; `git status` byte-identical before/after; HEAD unchanged).

```
R02 FINAL STAGE REVIEW
CANDIDATE: e876171c5cd0a6f8 (digest 96d5aa25…, 11409 files)
HEAD: cdd213078f6947217000c7ecd1a36ab5ffe2bb01 (worktree = authorized repairs, uncommitted)
```

## Independent verification performed (not read-only trust of the gate)

| # | Check | Command (real, executed by reviewer) | Result |
|---|---|---|---|
| 1 | Auth negative matrix | `cargo test --manifest-path rust/Cargo.toml --locked --offline -p lingxi-service --test auth_matrix -- --nocapture` | **24/24 ok, exit 0** (20:44:32Z); a05 series: forged principal ignored, no-credential 401/403 without side effects, expired/revoked device creds rejected, cross-principal misuse forbidden, loopback-token-over-nonlocal denied at policy layer |
| 2 | Storage failure injection | `-p lingxi-adapters --test storage_transactions` then `--test backup_faults` (separate invocations) | **7/7 ok and 2/2 ok, both exit 0** (20:44:52Z); crash-window commit boundaries, bounded queue full-not-drop, busy timeout explicit, checkpoint fault fails backup explicitly leaving no artifact, interrupted copy removes partial file |
| 3 | Event resume | `-p lingxi-service --test event_subscription -- --nocapture` | **12/12 ok, exit 0** (20:45:04Z); snapshot/subscription no gap at every commit boundary, join-window race loses nothing, expired/stale cursor -> explicit snapshot_required matching authority, intra-page seq gap is loud corruption, purge-resume race never silent hole |
| 4 | Shutdown/recovery | `-p lingxi-service --test instance_lifecycle -- --nocapture` + `bash scripts/rust-tauri/r02_t06_recovery_drill.sh /tmp/r02-reviewer-r1/a12` | lifecycle **3/3 ok exit 0**; drill **ALL GREEN exit 0** (20:45:41Z): v5 garbage main db -> exit=2 explicit refusal, corrupt file byte-identical, NO empty db substituted; v7 tampered migration receipt -> exit=2 SchemaTampered refusal; v1–v3 torn stamp/journal all refuse; residue-check 12/12 spawned processes reaped |
| 5 | A15 full chain | `bash scripts/rust-tauri/r02_t08_full_chain_smoke.sh /tmp/r02-reviewer-r1/A15` | **ALL GREEN exit 0** (20:46:03Z): boot1 health 200 minimal → unauth 401 → token 200 → writes committed (runId …_000001/…_000002, snapshotSeq=2, live event seq=3, events page head=4 count=4 contiguous) → SIGTERM exit 0, no leftover process, port refused → boot2 on same home: OLD token 401, NEW token 200, session readback runCount=2, events preserved head=4 → clean close, instance record removed |
| 6 | A16 default entry | Read full `…/A16/legacy-entry/summary.txt` + independent source read of `desktop/src/shared/rust-local-service.cjs`, `desktop/main.cjs`, `cli/args.ts` + live `node -e` | summary GREEN; **`rustDesktopEnabled({})` = false**, explicit `node` = false, explicit `rust` = true, invalid value throws; main.cjs line 1438 `if (rustDesktopEnabled())` guards the Rust branch, line 1333 `RUST_DESKTOP_NODE_SERVER_INFO_PRESENT` mutex; package.json main = desktop/bootstrap.cjs (E1c); cli/args.ts line 18 `runtime: "node"` |
| 7 | Candidate binding | Python recomputation of the xtask candidate digest over `git ls-files --cached --others --exclude-standard` minus the gate evidence root | **digest = 96d5aa252c59dddced6731501e3c7016414bb5ce30a5d553d84c742ef6afb846, fileCount = 11409** (exact match); candidate-ID formula reproduces **e876171c5cd0a6f8** → zero drift between the gated candidate and the tree I tested |
| 8 | Evidence freshness | `verify-stage-result.json` bindings + dirs `a05_a06_auth_matrix`, `A05_A06` (29 files), `supplemental_management_matrix`/`MANAGEMENT` (10 files, 67 cases PASS) | before==after digest, `stable=true`, 20/20 checkpoints stable; stdout/stderr/evidence files exist, sizes/mtime (20:24–20:37 UTC) inside the gate window; all 9 deferred leaves listed with requirement=REQUIRED_SUPPLEMENTAL, non-empty r07Share, deferredToStage=R07 |
| 9 | Boundary integrity | 3 leaves × 11 mirror fields vs both R00 ledgers; set equality; command refs | D3710D637C19 / 200D4E5D52C9 / B8A1AD32A8E1: **all 11 r00* mirror fields verbatim equal** to `ACCEPTANCE_MAP.json` + `FEATURE_STAGE_ACCEPTANCE.json`; stage-map 34 == R02-bound set in ACCEPTANCE_MAP (34), all present in FSA; 9 deferred all REQUIRED_SUPPLEMENTAL; 16 base scenarios all REQUIRED, every commandRef resolves, all 20 commands referenced; sessions leaf split = 6 gating server cases + 4 CLI cases deferredR07 (all OBSERVED_HELD, non-gating) |

## Gate evidence relied upon (r6, independently cross-checked)

`artifacts/rust-tauri/R02/final-candidate-e876171c5cd0a6f8/verify-stage-result.json`: overall PASS;
testedSha cdd21307…; candidateSourceBinding stable (before==after==96d5aa25…, 11409 files, 20 checkpoints);
runnerSourceBinding compiled==disk for the 9 xtask source/manifest files; 16/16 scenarios PASS;
34 leaves = 25 PASS + 9 DEFERRED_TO_R07 + 0 FAIL + 0 BLOCKED; 20/20 commands exit 0 with no
preExisting/missing/timedOut. Pre-steps in `/tmp/r02-final/gate-r6/` (12 logs, all exit 0): fmt, clippy,
per-crate test partition protocol 20 + kernel 12 + adapters 53 + service 262 + xtask 87 + spike 7 +
browser-spike 12 = **453 tests, 0 failed** (the documented per-crate partition that avoids the known
local macOS LAN self-connect interference — environmental, not a product defect), check-contracts,
check-boundaries, verify-stage. Historical failed-round evidence roots (a0af8366…, a29e5adb…,
fc997189…) retained unmodified as required.

```
R02-T01..T08:
  R02-T01 workspace/组合根            PASS (A01 real build + desktop-free dep tree: 315-pkg resolve graph,
                                      0 hits for tauri/electron/tao/wry/webkit2gtk/winit/webview;
                                      real process start, health 200, SIGTERM clean; A02 negative gate exit 0)
  R02-T02 配置/路径/单写者            PASS (A03 dual-instance rejection + A04 path priority exit 0;
                                      reviewer rerun instance_lifecycle 3/3: locked instance survives
                                      rejected second claim; crashed owner stale record restart takeover)
  R02-T03 HTTP/WS 认证与资源范围      PASS (gate a05_a06 exit 0; reviewer rerun auth_matrix 24/24;
                                      forged principal/session/wrong-token all rejected without side effects;
                                      Origin/Host/expired-ticket matrix green)
  R02-T04 存储 ports 与事务           PASS (gate a07_a08 + live fault pair exit 0; reviewer rerun
                                      storage_transactions 7/7 + backup_faults 2/2 — crash windows, busy
                                      timeout, checkpoint fault, interrupted backup)
  R02-T05 事件顺序/快照/续读          PASS (gate a09_a10 exit 0; reviewer rerun event_subscription 12/12 —
                                      no-gap snapshot at every commit boundary, snapshot_required on
                                      expired/stale cursor, dedup, loud seq-gap corruption)
  R02-T06 备份/关闭/启动恢复          PASS (gate a11 exit 0; reviewer reran the full T06 drill: ALL GREEN —
                                      corrupt db / torn stamp / torn journal / tampered receipt all
                                      explicitly refused with byte-identical files, no auto-discard,
                                      no empty-db substitution, 12/12 processes reaped)
  R02-T07 日志/错误/有界资源          PASS (gate a13 redaction scan + a14 slow subscriber exit 0;
                                      limits enforced in auth_matrix body/rate/WS-ceiling case)
  R02-T08 独立服务交付与门禁          PASS (reviewer reran A15 ALL GREEN and A16 chains live; xtask
                                      verify-stage executes real commands with F04 stale-evidence refusal,
                                      F05 timeout cleanup, empty-set/unknown-stage negative tests in the
                                      87 xtask tests)

R02-A01..A16: A01 PASS, A02 PASS, A03 PASS, A04 PASS, A05 PASS, A06 PASS, A07 PASS, A08 PASS,
              A09 PASS, A10 PASS, A11 PASS, A12 PASS, A13 PASS, A14 PASS, A15 PASS, A16 PASS
              (each backed by a gate command exit 0 + evidence dir; A05/A06, A07/A08, A09/A10, A12,
              A15 additionally re-executed by this reviewer; A16 independently re-derived from source
              + live node -e)

SUPPLEMENTAL R02 OBLIGATIONS: PASS (25/25 r02_share_satisfied leaves PASS in the r6 result; spot
  verification of leaf-cases.json pins shows actual==expect for the sampled leaf; R07 remainder carried
  on every share leaf via deferredToStage=R07 + non-empty r07Share)

R07 DEFERRED OBLIGATIONS (9, all still REQUIRED_SUPPLEMENTAL, none weakened/deleted):
  - R00-T02-LA-B8A1AD32A8E1 (CLI help): full CLI help behavior — print commands/args exit 0, unknown
    arg error+help exit 1, no service start
  - R00-T02-LA-32FFEC05BAA7 (UI settings sharing): full sharing-settings page behavior (init
    cache/15s timeout/error ready, three-color card, width card, font/limit projections, explicit
    failure when storage forbidden)
  - R00-T02-LA-8BC1A036AFAA (static hosting desktop): R07-T09 must re-verify full static hosting on
    its then-current artifacts (content types/permissions, missing-dist guidance, invalid dist 503,
    traversal refusal, oversize explicit error); implemented static_web.rs must be kept healthy
  - R00-T02-LA-3291CFD5F7E2 (static hosting mobile): same R07-T09 static-hosting obligations
  - R00-T02-LA-2A1C298F62FC (mobile bootstrap): full mobile init entry (agent/language/avatar/
    workspace/preferences, expiry cleanup, no fake empty state on failure)
  - R00-T02-LA-8ED658F9DB9E / F8935B6B0221 / 39AD35E1FD71 (thinking-level set/default/index-api
    session): full entry behavior — per-session/new-session default level read+set, query boundary
    (sessionPath/pendingNewSession) without widened disclosure, invalid level/409 no state change
  - R00-T02-LA-000E6E1301C0 (UI settings access): R07/R08 full UI action matrix (container init,
    access overview+QR, LAN/port/public address save, credential generation+QR, copy/connect remote,
    revoke device/credential, profile/password edit — success/empty/error triads)

PRODUCTION DEFAULT: Node/Electron (verified three ways: E1c/E1d assertions in the A16 gate run;
  live source read of rust-local-service.cjs default 'node' branch, main.cjs guard, cli/args.ts
  default; reviewer-executed node -e proving rustDesktopEnabled default false, opt-in rust true,
  invalid value throws. No LINGXI_DESKTOP_SERVER_RUNTIME injection anywhere in the entry chain.)

FINDINGS: NONE (no BLOCKING, MAJOR, or MINOR product findings)

Post-PASS obligations (known, not findings per review charter):
  1. docs/rust-tauri/R02/{R02_HANDOFF.json, R02_ACCEPTANCE_LEDGER.json, R02_REPORT.md} still bind the
     old candidate (status R18_FAIL…, source_sha 9b98c679…). Orchestrator must refresh them to
     e876171c5cd0a6f8 / digest 96d5aa25… / overall PASS before closeout.
  2. A16 candidate npm test raw exit 1 = the registered seal trio (post-verification-audit-seal +
     round2/round3-delivery-evidence), classified as seal-coordinate governance lag per the seal
     workflow, present at HEAD binding rules, not an R02 regression (pristine-base replay is green;
     no new reds vs baseline ∪ registered families).
  3. Uncommitted authorized repairs (21 tracked + 4 evidence roots) must be committed with the
     evidence bound to the actual commit before any seal step (per PROGRESS.md seal workflow; the
     audit-seal whitelist check will otherwise fail on coordinate lag — governance, not product).

VERDICT: PASS
```

## Answers to the five mandatory questions

1. **Rust independence — YES.** The reviewer built and ran the real `lingxi-service` binary from
   cargo (--locked --offline) with no Tauri/Electron present: start → health → authenticate →
   write → subscribe → SIGTERM → restart → read-back all green on a synthetic home; A01 evidence
   shows the 315-package resolve graph with zero desktop-dependency hits.
2. **Single source of truth — YES.** Rust owns its own runtime dir `{home}/lingxi-service/`
   (own instance.json/lock, 0700), Node server-info.json presence blocks Rust startup
   (RUST_DESKTOP_NODE_SERVER_INFO_PRESENT) and the desktop Rust path is opt-in-only, so no dual
   writers on one home; terminal state + key events commit in one transaction with post-commit
   publication (A07 fault injection, rerun); event seq is produced by the single writer per stream
   (A09/A10, rerun 12/12). No second completion-state/identity/event-sequence system: R02 implements
   the service side only; production identity/events remain with the Node incumbent by default.
3. **Evidence freshness — YES.** Every PASS in the r6 result binds candidate e876171c5cd0a6f8 with
   digest 96d5aa25… (before==after, 20/20 checkpoints stable); the reviewer recomputed the digest
   over the current tree and got the identical value — no drift between the gated candidate and the
   reviewed tree, so no old-source result is being passed off as current.
4. **R02/R07 boundary — HELD.** No R03 agent-loop, R05 provider integration (the only "anthropic"
   string in rust/ is a REDACTED example fixture in the protocol schema generator), or full R07
   client migration was smuggled into R02: the stage map splits each dual-stage leaf into an R02
   service-side share (25, gated) and an R07 remainder; the 9 leaves with no R02 share are
   DEFERRED_TO_R07, still REQUIRED, with non-empty r07Share obligations, verified equal to the
   stage-map declarations; CLI `--runtime rust` is explicit opt-in with default node.
5. **A16 — PASS.** Production default entry remains Node/Electron (package.json main =
   desktop/bootstrap.cjs; runtime default 'node' at desktop and CLI; Rust branch fully guarded);
   the Rust preview is explicit opt-in via LINGXI_DESKTOP_SERVER_RUNTIME=--runtime rust and cannot
   double-write (mutex + separate runtime dir). Legacy regressions: none beyond the registered
   seal-trio governance family (classified, base-replay-verified).

## Scope limitations of this review

- All reviewer reruns executed on this macOS arm64 host, cargo 1.98.1, --locked --offline; no other
  platform, no packaged/installer build, no real external provider (none is in R02 scope).
- Full per-crate workspace tests were not re-run by the reviewer in one shot (known local macOS
  LAN self-connect interference); the r6 gate's per-crate partition (453 tests, 0 failed) was
  cross-checked log-by-log, and the reviewer independently re-executed the five highest-risk
  suites plus both end-to-end scripts.
- App-based seal/audit workflow steps remain with the orchestrator (see post-PASS items).
