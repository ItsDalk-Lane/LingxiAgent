# E01-state first execution: hang postmortem (fact preservation)

E02's first real execution of E01's delivered test suite
(`cargo test -p lingxi-service --test r04_t07_mcp_and_workers`,
no code changes yet) produced ZERO output for ~8 minutes. The cargo
process tree was still alive:

    ps: r04_t07_mcp_and_workers-<hash> running, no test output captured

macOS `sample <pid>` (2s) showed the main thread parked in
run_tests_console recv and exactly ONE test thread alive:

    Thread_<t1>: adversarial_mcp_pagination_walks_all_pages
      -> tokio Runtime::block_on
      -> register_mcp_server
      -> sync_server_tools
      -> McpServer::list_tools
      -> rmcp Peer<RoleClient>::list_tools
      -> send_request (waiting for the server's page)

Root cause (E01 fixture bug, confirmed in source): the synthetic paged
server's cursor logic computed the next page offset as

    let offset = if cursor == "start" { 0 } else { size };

so EVERY non-first page returned tools[size..2*size] again with
next_cursor = Some("more") — the bridge's listing walk (which follows
cursors until None) never terminated: an infinite tools/list request
loop, not a lockup.

E02 fixes (see R04-T07_REPORT.md §2.3 #1):
1. fixture cursor = numeric offset (deterministic page walk);
2. bridge-side bounds MCP_MAX_LIST_PAGES (64) / MCP_MAX_LISTED_TOOLS
   (1024) — the same attack shape from a genuinely malicious untrusted
   server is now a loud refusal (new adversarial test
   adversarial_endless_paging_is_refused_loudly).

After the fix the suite completes in ~2s (19/19).
