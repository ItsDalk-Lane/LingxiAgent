# R04-T06 pre-implementation real probes (design evidence, 2026-09-30)

Executed on the macOS host (darwin 27.0.0 arm64) BEFORE writing the Rust
code, to establish the real semantics the port had to preserve. Every
probe ran in an isolated `mktemp -d` directory created and removed by the
probe itself.

## P1 — seatbelt judges the REAL path (realpath embedding is mandatory)

Profile with the writable subpath embedded as the UNRESOLVED `/tmp/...`
path:

```
$ /usr/bin/sandbox-exec -p '(version 1)(deny default)…(allow file-write* (subpath "/tmp/<probe>/ws"))(allow network-outbound)' \
    -- /bin/sh -c "echo ok > /tmp/<probe>/ws/inside.txt"
/bin/sh: /tmp/<probe>/ws/inside.txt: Operation not permitted
```

The same profile with the REALPATH'd `/private/tmp/<probe>/ws` subpath:
write succeeds (`ok`). This is why the incumbent `seatbelt.ts` realpaths
every embedded path — and why the Rust port must too (`realpath_or_lexical`).

## P2 — symlink escape through a writable-root link is DENIED (real target)

`ws/link.txt -> restricted/s.txt` (restricted is OUTSIDE every writable
root); writing through the link under the seatbelt profile:

```
/bin/sh: /private/tmp/<probe>/ws/link.txt: Operation not permitted
```

`restricted/s.txt` kept `TOPSECRET`. Seatbelt judges the write by its
real destination — the link does not smuggle writes out of the writable
root.

## P3 — network allow vs deny against a self-created loopback listener

A listener bound to 127.0.0.1:0 (created by the probe, port 52008):

- profile with `(allow network-outbound)` → `nc` receives `pong`;
- profile with `(deny network*)` → no data, connection denied.

## P4 — sandbox-exec execs in place (pid preserved)

`sandbox-exec -p <profile> -- /bin/sh -c 'echo pid=$$'` reported the
SAME pid as the process the parent spawned ($! == $$): sandbox-exec
REPLACES itself with the command. Consequence: the T05 supervisor's
`setsid` (pre_exec) + `getpgid` verification + `killpg` chain keeps
working through the wrapper.

## P5 — group kill reaches grandchildren through the wrapper

Parent forked a child that `setsid`'d (the supervisor's pre_exec shape),
exec'd `sandbox-exec -p <profile> -- /bin/sh -c 'sleep 30 & echo $! > gc.pid; wait'`,
then `killpg(child, SIGTERM)`:

```
child(wrapper)= 6258 pgid= 6258 grandchild= 6259
GC KILLED by group killpg - supervisor chain works through sandbox-exec
```

(The grandchild sleep was a probe-created process; nothing else was
signaled.)

## P6 — helper availability facts (the chosen trust scheme's basis)

- `/usr/bin/sandbox-exec` exists: regular file, root:wheel owned,
  mode 0755 (no group/world write bit) — the metadata trust checks in
  `verify_helper_metadata` hold for the pinned system helper.
- `sandbox-exec --version` does NOT exist (`illegal option -- -`); the
  usage line lists exactly `-f/-n/-p/-D` — hence the chosen scheme:
  identity pinning + metadata trust + the two-legged enforcement probe
  (there is no version interface to parse on this backend).
- The enforcement probe's containment core
  (`process/mach/ipc/sysctl/reads allowed, everything else denied`)
  satisfies both legs by hand:
  - `/usr/bin/true` under it → exit 0;
  - `/bin/sh -c 'true > /dev/null'` under it → `Operation not permitted`,
    exit 1.
  (A bare `(deny default)+(allow process-exec*)` aborts even a plain
  exec — SIGABRT — which is why the probe uses the containment core.)

These probes are the reason the implementation embeds realpaths, refuses
quote/backslash paths (SBPL literal escape = policy injection), keeps the
deny lines after the allows (last-match-wins), and relies on the
supervisor's group-kill through the exec'ing wrapper.
