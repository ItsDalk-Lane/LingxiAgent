#!/usr/bin/env python3
"""R05 RR1 F26: rebuild the R05 stage tables from the CURRENT tree.

Inputs (all machine-verified):
- the per-suite test inventories (cargo test -- --list, captured fresh);
- the OLD cid table (existing ownership is kept verbatim — a pre-existing
  (cid, run, test) binding only changes if the test itself moved);
- the NEW mappings for every test RR1 added (the rr1_* batteries) and the
  new suites (r05_t05_network, r05_t06_rr1_*, r05_t07_rr1_usage_ledger,
  r05_t08_production_tools, r05_t08_resources, adp r05_t03_rr1_replay,
  adp r05_t04_rr1_batch_terminal, adp r05_t07_rr1_usage_strict).

Outputs:
- docs/rust-tauri/R05/r05_stage_pins.tsv  (suite pins with EXACT counts);
- docs/rust-tauri/R05/r05_stage_cids.tsv  (test ownership, one cid per test);
- docs/rust-tauri/R05/r05_required_cids.tsv (the AUTHORITATIVE required-C-ID
  registry: 100 original C-IDs from the 2026-10-02 special prompt appendix
  + the 3 appended R05 C-IDs, each with its binding kind — `cid` (owned by
  the cid table) or `command:<key>` (bound to a registered stage-map
  command; the shared execution is valid evidence, the relationship is
  documented in R05_TEST_MAP.json)).

The xtask mirror (stage_map.rs) and the assembler (r05_t08_stage_suites.sh)
both verify against r05_required_cids.tsv — a fabricated/renamed CID or a
dropped registry line turns the workspace test suite and the producer red.

Run:  python3 scripts/rust-tauri/r05_t08_rebuild_tables.py --inventories <dir>
      (the dir holds one `<run-key>.tests` file per suite, one name/line)
"""
import argparse
import pathlib
import sys

REPO = pathlib.Path(__file__).resolve().parents[2]
R05 = REPO / "docs/rust-tauri/R05"

# ── the authoritative required C-ID registry (103 = 100 original + 3) ────────
# Sources: RR1_MASTER_PROMPT_2026-10-04.md 附录B (the 100 original C-IDs,
# themselves the 2026-10-02 special prompt's checklist) + the 3 appended
# C-IDs already registered in r05_stage_cids.tsv / R05_TEST_MAP.json.
ORIGINAL_CIDS = []
for task, count in [("T01", 12), ("T02", 12), ("T03", 12), ("T04", 16),
                    ("T05", 12), ("T06", 12), ("T07", 10), ("T08", 14)]:
    for i in range(1, count + 1):
        ORIGINAL_CIDS.append(f"R05-{task}-C{i:02d}")
APPENDED_CIDS = ["R05-T05-C11B", "R05-T05-C13", "R05-T06-C11B"]
ALL_REQUIRED = ORIGINAL_CIDS + APPENDED_CIDS
assert len(ORIGINAL_CIDS) == 100, len(ORIGINAL_CIDS)
assert len(ALL_REQUIRED) == 103

# Bindings of the C-IDs WITHOUT cid-owned cargo tests. Every entry names a
# REGISTERED stage-map command key (verified by the xtask mirror); the
# specific producer tests of each shared execution are documented in
# R05_TEST_MAP.json (F28 refresh). Everything else is cid-owned.
COMMAND_BOUND = {
    # 范围矩阵证据（三份矩阵生成脚本重跑 + F25 的 xtask 镜像覆盖核验）
    "R05-T01-C01": "rust_test_workspace",
    # 依赖方向与范围不倒退（check-boundaries 即其门禁生产者）
    "R05-T01-C12": "check_boundaries",
    # 重试身份及实际请求可对账（run_lifecycle 重试腿，workspace 内真实运行）
    "R05-T05-C04": "rust_test_workspace",
    # 部分文本重试不重复拼接（r05_t05_timeouts + run_lifecycle）
    "R05-T05-C06": "rust_test_workspace",
    # 取消覆盖每类等待/唯一结算（cancellation_tree/cancel_terminal_race/
    # cancel_link_inheritance，workspace 内真实运行）
    "R05-T05-C09": "rust_test_workspace",
    # 客户端重连不重新请求模型（background_disconnect_recovery/event_subscription）
    "R05-T05-C10": "rust_test_workspace",
    # 与 R05-T05-C11B 同一 egress 测试组的 additional citations（归属唯一化）
    "R05-T06-C11B": "rust_test_workspace",
    # 媒体最小链（adp r05_t06_operations 的 c10 腿；与 T06-C12 共享一次执行）
    "R05-T08-C10": "rust_test_workspace",
    # R04 与 R03 真实回归（verify-stage R04 内嵌闭包）
    "R05-T08-C13": "r04_regression_gate",
    # 范围和默认旧产品不变（边界检查 + workspace 测试）
    "R05-T08-C14": "check_boundaries",
}

# ── new ownership for every test RR1 added (suite run key -> [(test, cid)]) ──
NEW_SUITES = {
    "svc:r05_t05_network": "R05-T05-",
}
NEW_OWNERSHIP = {
    # svc:r05_t01_model_plane — the F01/F02/F29 batteries
    "svc:r05_t01_model_plane": [
        ("rr1_f01_f02::f01_control_no_capability_needs_still_dispatches", "R05-T01-C06"),
        ("rr1_f01_f02::f01_declared_image_capability_sends_the_image_payload", "R05-T01-C06"),
        ("rr1_f01_f02::f01_declared_tools_capability_sends_the_tools_payload", "R05-T01-C06"),
        ("rr1_f01_f02::f01_explicitly_unsupported_tools_capability_makes_zero_requests", "R05-T01-C06"),
        ("rr1_f01_f02::f01_undeclared_image_capability_makes_zero_requests", "R05-T01-C06"),
        ("rr1_f01_f02::f01_undeclared_tools_capability_makes_zero_requests", "R05-T01-C06"),
        ("rr1_f01_f02::f01_keyless_local_model_with_declared_tools_works", "R05-T01-C11"),
        ("rr1_f01_f02::f01_reload_removing_the_declaration_or_binding_refuses_new_calls", "R05-T01-C05"),
        ("rr1_f01_f02::f01_same_model_id_across_providers_and_auxiliary_slot_follow_the_route", "R05-T01-C04"),
        ("rr1_f01_f02::f02_kept_seed_serves_both_generations", "R05-T01-C05"),
        ("rr1_f01_f02::f02_key_only_rotation_safe_failure_then_new_call_uses_new_key", "R05-T01-C05"),
        ("rr1_f01_f02::f02_old_route_never_resolves_new_generation_material", "R05-T01-C05"),
        ("rr1_f01_f02::f02_reload_during_401_never_sends_new_key_to_old_endpoint", "R05-T01-C05"),
        ("rr1_f01_f02::f02_removed_provider_old_route_is_not_configured", "R05-T01-C05"),
        ("rr1_f01_f02::f02_resolve_dispatch_freezes_compat_and_capabilities_with_the_route", "R05-T01-C05"),
        ("rr1_f01_f02::f02_unrelated_provider_reload_is_isolated_per_provider", "R05-T01-C05"),
        ("rr1_f29::f29_management_reload_during_inflight_401_never_leaks_the_new_key", "R05-T02-C10"),
    ],
    # svc:r05_t02_credentials — F03/F05 handles+secrets, F04 OAuth battery
    "svc:r05_t02_credentials": [
        ("rr1_t02::rr1_f03_handles_are_bound_to_the_minting_principal", "R05-T02-C12"),
        ("rr1_t02::rr1_f03_hot_added_oauth_provider_seeds_tokens_and_survives_restart", "R05-T02-C11"),
        ("rr1_t02::rr1_f03_old_handle_must_not_resolve_reloaded_secret", "R05-T02-C05"),
        ("rr1_t02::rr1_f03_removed_then_readded_provider_must_not_revive_old_handle", "R05-T02-C05"),
        ("rr1_t02::rr1_f05_encoded_echos_of_the_material_are_scrubbed", "R05-T02-C09"),
        ("rr1_t02::rr1_f05_protocol_error_echo_must_not_carry_complete_secret_into_kernel", "R05-T02-C09"),
        ("rr1_t02::rr1_f05_provider_echo_full_chain_never_reaches_durable_state", "R05-T02-C09"),
        ("rr1_t02::rr1_f05_retry_path_scrubs_both_the_stale_and_the_fresh_material", "R05-T02-C09"),
        ("rr1_t02::rr1_f05_truncation_never_exposes_a_credential_prefix_at_any_boundary", "R05-T02-C09"),
        ("rr1_f04::rr1_f04_add_custom_model_id_refreshes_and_persists", "R05-T02-C08"),
        ("rr1_f04::rr1_f04_browser_completion_then_manual_replay_refused_zero_new_exchanges", "R05-T02-C08"),
        ("rr1_f04::rr1_f04_device_denial_never_reports_logged_in", "R05-T02-C08"),
        ("rr1_f04::rr1_f04_device_start_poll_done_then_logout", "R05-T02-C08"),
        ("rr1_f04::rr1_f04_expired_state_refuses_even_with_the_correct_state", "R05-T02-C08"),
        ("rr1_f04::rr1_f04_login_midflight_revoke_and_reload_fence_late_installs", "R05-T02-C08"),
        ("rr1_f04::rr1_f04_logout_clears_credentials_cache_and_refreshes_models", "R05-T02-C08"),
        ("rr1_f04::rr1_f04_oauth_model_listing_and_non_oauth_rejection", "R05-T02-C08"),
        ("rr1_f04::rr1_f04_pkce_browser_callback_listener_completes_the_login", "R05-T02-C08"),
        ("rr1_f04::rr1_f04_pkce_start_manual_callback_install_and_real_model_call", "R05-T02-C08"),
        ("rr1_f04::rr1_f04_remove_custom_model_id_refreshes_the_list", "R05-T02-C08"),
        ("rr1_f04::rr1_f04_stale_completion_after_restart_never_eats_the_new_transaction", "R05-T02-C08"),
        ("rr1_f04::rr1_f04_status_reports_logged_in_and_available_model_counts", "R05-T02-C08"),
    ],
    # svc:r05_t03_protocol_adapters — the F08 service-level legs
    "svc:r05_t03_protocol_adapters": [
        ("rr1_f08_deepseek_reasoning_replays_onto_the_next_wire_request", "R05-T03-C12"),
        ("rr1_f08_missing_required_reasoning_fails_closed_before_the_wire", "R05-T03-C06"),
    ],
    # svc:r05_t04_streaming — F11/F12/F13 batteries
    "svc:r05_t04_streaming": [
        ("rr1_f11_legal_then_schema_invalid_batch_has_zero_side_effects", "R05-T04-C05"),
        ("rr1_f11_identical_resend_executes_once", "R05-T04-C10"),
        ("rr1_f11_same_provider_id_conflict_is_loud_with_zero_side_effects", "R05-T04-C10"),
        ("rr1_f11_distinct_ids_with_equal_arguments_both_execute", "R05-T04-C09"),
        ("rr1_f12_mood_only_text_settles_process_only_without_final", "R05-T04-C11"),
        ("rr1_f12_reasoning_only_turn_settles_process_only_without_final", "R05-T04-C11"),
        ("rr1_f12_unclassified_live_text_is_not_marked_final_pre_terminal", "R05-T04-C11"),
        ("rr1_f12_unclassified_live_text_stays_unresolved_until_the_terminal", "R05-T04-C11"),
        ("rr1_f13_final_message_projection_is_normalized_same_source", "R05-T04-C13"),
    ],
    # svc:r05_t08_closed_loop — the F13 restart leg lives in the closed-loop
    # suite (it drives the REAL binary through SIGTERM + restart)
    "svc:r05_t08_closed_loop": [
        ("rr1_f13_normalized_final_survives_restart_on_the_real_binary", "R05-T08-C09"),
    ],
    # svc:r05_t07_persistence — the v6→v7 transport_attempts migration leg
    "svc:r05_t07_persistence": [
        ("migration_v6_to_v7_makes_attempts_nullable_and_keeps_rows", "R05-T07-C02"),
    ],
    # ── the NEW RR1 suites ──────────────────────────────────────────────────
    "svc:r05_t05_network": [
        ("rr1_f14::direct_policy_dials_the_source_itself", "R05-T05-C12"),
        ("rr1_f14::forced_loopback_bypass_keeps_local_endpoints_direct", "R05-T05-C12"),
        ("rr1_f14::manual_proxy_routes_a_real_chat_turn_through_the_proxy", "R05-T05-C12"),
        ("rr1_f14::network_policy_section_parses_and_validates", "R05-T05-C12"),
        ("rr1_f14::oauth_operation_and_download_share_the_proxy_policy", "R05-T05-C12"),
        ("rr1_f14::private_ca_authorizes_chat_and_default_verification_refuses_it", "R05-T05-C12"),
        ("rr1_f14::publishing_a_policy_generation_reroutes_the_next_request", "R05-T05-C12"),
        ("rr1_f14::system_mode_routes_through_the_snapshotted_environment", "R05-T05-C12"),
        ("rr1_f14::wrong_hostname_expired_and_wrong_chain_are_refused", "R05-T05-C12"),
        ("rr1_f15::a_proxied_download_routes_through_the_proxy_and_still_refuses_guarded_names", "R05-T05-C11"),
        ("rr1_f15::localhost_download_makes_zero_connections", "R05-T05-C11"),
        ("rr1_f15::mixed_candidates_refuse_and_public_candidates_pass_judgment", "R05-T05-C11"),
        ("rr1_f15::private_resolved_candidate_refuses_with_zero_connections", "R05-T05-C11"),
        ("rr1_f15::the_pinned_dial_never_re_resolves", "R05-T05-C11"),
        ("rr1_f15::the_same_origin_exception_downloads_real_bytes", "R05-T05-C11"),
        ("rr1_f16::a_parked_refresh_wait_obeys_the_call_deadline", "R05-T05-C01"),
        ("rr1_f16::chat_queue::the_run_drivers_queue_wait_obeys_the_call_budget", "R05-T05-C01"),
        ("rr1_f16::error_body_must_obey_total_deadline", "R05-T05-C01"),
        ("rr1_f16::error_body_read_failure_is_preserved_not_swallowed", "R05-T05-C01"),
        ("rr1_f16::operations_queue_wait_obeys_the_call_budget", "R05-T05-C01"),
        ("rr1_f16::oversized_error_body_is_capped_with_the_fact_preserved", "R05-T05-C01"),
    ],
    "svc:r05_t06_rr1_media_resource": [
        ("rr1_f17_attachment_read_uses_the_authorized_canonical_path", "R05-T06-C03"),
        ("rr1_f17_attachment_size_caps_refuse_loudly", "R05-T06-C03"),
        ("rr1_f17_leaf_symlink_to_outside_is_refused_by_real_target", "R05-T06-C03"),
        ("rr1_f17_legal_attachment_reads_still_work", "R05-T06-C03"),
        ("rr1_f17_special_file_is_refused_not_read_unbounded", "R05-T06-C03"),
        ("rr1_f37_parent_dir_swapped_to_inside_symlink_reads_the_real_target", "R05-T06-C03"),
        ("rr1_f37_parent_dir_swapped_to_outside_symlink_is_refused_by_real_target", "R05-T06-C03"),
        ("rr1_f18_cross_kind_task_id_is_refused", "R05-T06-C02"),
        ("rr1_f18_duplicate_provider_task_id_is_disambiguated", "R05-T06-C02"),
        ("rr1_f18_f19_dashscope_image_task_still_completes_via_registered_product", "R05-T06-C02"),
        ("rr1_f18_legacy_fallback_uses_the_distinct_host_tracker_id", "R05-T06-C02"),
        ("rr1_f18_old_task_survives_reload_without_retargeting", "R05-T06-C02"),
        ("rr1_f18_video_poll_sends_the_provider_job_id", "R05-T06-C02"),
        ("rr1_f19_cancel_after_completion_keeps_the_delivered_fact", "R05-T06-C12"),
        ("rr1_f19_cancel_during_download_leaves_no_registered_product", "R05-T06-C12"),
        ("rr1_f19_cancel_during_inflight_poll_fences_download_and_completion", "R05-T06-C12"),
        ("rr1_f19_concurrent_polls_deliver_once", "R05-T06-C12"),
    ],
    "svc:r05_t06_rr1_system_speech": [
        ("rr1_f20_dropped_future_kills_process_and_cleans_output", "R05-T06-C12"),
        ("rr1_f20_empty_text_refuses_before_spawn", "R05-T06-C12"),
        ("rr1_f20_exhausted_deadline_refuses_before_spawn", "R05-T05-C01"),
        ("rr1_f20_leading_dash_text_is_message_not_option", "R05-T06-C12"),
        ("rr1_f20_missing_supervisor_refuses_fail_closed", "R05-T06-C12"),
        ("rr1_f20_queued_speech_waits_for_the_unified_permit", "R05-T05-C02"),
        ("rr1_f20_real_say_registers_product_with_zero_transport_attempts", "R05-T06-C12"),
        ("rr1_f20_short_deadline_times_out_kills_and_cleans", "R05-T05-C01"),
        ("rr1_f20_unknown_voice_falls_back_and_delivers", "R05-T06-C12"),
        ("rr1_f20_unwritable_output_fails_honestly", "R05-T06-C12"),
    ],
    "svc:r05_t07_rr1_usage_ledger": [
        ("rr1_f21_empty_answer_failure_keeps_the_reported_usage", "R05-T07-C05"),
        ("rr1_f21_operation_context_carries_session_run_and_cause", "R05-T07-C02"),
        ("rr1_f21_operation_queue_timeout_never_invents_an_http_attempt", "R05-T07-C03"),
        ("rr1_f21_partial_usage_survives_a_failed_aux_settlement", "R05-T07-C05"),
        ("rr1_f21_unexpected_tool_response_failure_keeps_the_reported_usage", "R05-T07-C05"),
        ("rr1_f21_usage_query_filters_by_date_purpose_and_model", "R05-T07-C09"),
        ("rr1_f21_worker_failed_physical_request_must_leave_unknown_usage_row", "R05-T07-C03"),
        ("rr1_f21_worker_parent_join_through_the_real_subprocess_chain", "R05-T07-C02"),
        ("rr1_f21_worker_pre_send_refusal_records_zero_attempts", "R05-T07-C03"),
        ("rr1_f22_operation_invalid_usage_must_not_persist_secret_payload", "R05-T07-C09"),
        ("rr1_f38_driver_cancel_of_in_flight_stream_leads_a_cancelled_row_across_restart", "R05-T07-C03"),
        ("rr1_f38_run_cancel_drop_of_in_flight_callback_leads_a_cancelled_row", "R05-T07-C03"),
        ("rr1_f38_worker_deadline_expiry_of_in_flight_callback_leads_a_cancelled_row", "R05-T07-C03"),
        ("rr1_f39_fence_cancelled_before_write_keeps_the_turns_real_usage_row", "R05-T07-C03"),
        ("rr1_f39_fence_mismatch_cancelled_during_audit_keeps_the_turns_real_usage_row", "R05-T07-C03"),
    ],
    "svc:r05_t08_production_tools": [
        ("f24_production_wire_declares_the_four_tools", "R05-T01-C08"),
        ("f24_exec_command_runs_a_real_subprocess_end_to_end", "R05-T08-C01"),
        ("f24_exec_command_workdir_outside_the_workspace_is_refused", "R05-T08-C03"),
        ("f24_worker_nested_chain_under_global_permit_one", "R05-T08-C11"),
        ("f24_worker_child_environment_carries_no_credential_material", "R05-T06-C11"),
        ("f24_sigterm_with_parked_callback_reaps_the_worker_child", "R05-T06-C10"),
    ],
    "svc:r05_t08_resources": [
        ("f27_sampler_controls_detect_growth_release_and_failure", "R05-T08-C12"),
        ("f27_sustained_cancel_error_worker_load_stays_bounded", "R05-T08-C12"),
    ],
    "adp:r05_t03_rr1_replay": [
        ("rr1_f06_buffered_empty_thinking_preserves_signature_on_next_request", "R05-T03-C10"),
        ("rr1_f06_byte_split_stream_keeps_the_signed_thinking_block", "R05-T03-C10"),
        ("rr1_f06_stream_absent_placeholder_signature_accepts_the_final_delta", "R05-T03-C10"),
        ("rr1_f06_stream_identical_signature_resend_is_tolerated", "R05-T03-C10"),
        ("rr1_f06_stream_placeholder_signature_accepts_the_final_signature_delta", "R05-T03-C10"),
        ("rr1_f06_stream_two_different_final_signatures_are_a_true_conflict", "R05-T03-C10"),
        ("rr1_f07_consecutive_rounds_and_reverse_results_group_per_round", "R05-T03-C03"),
        ("rr1_f07_empty_text_signature_part_round_trips_verbatim", "R05-T03-C10"),
        ("rr1_f07_f10_unmatched_or_duplicated_anchors_fail_loudly", "R05-T03-C10"),
        ("rr1_f07_function_call_signature_survives_both_wire_modes", "R05-T03-C10"),
        ("rr1_f07_interleaved_parts_keep_original_order", "R05-T03-C06"),
        ("rr1_f07_parallel_results_form_one_user_content", "R05-T03-C03"),
        ("rr1_f08_buffered_reasoning_content_reaches_the_canonical", "R05-T03-C06"),
        ("rr1_f08_deepseek_reasoning_replays_through_renderer_and_compat", "R05-T03-C06"),
        ("rr1_f08_missing_required_reasoning_fails_closed_before_dispatch", "R05-T03-C06"),
        ("rr1_f08_no_contract_provider_never_receives_the_carrier", "R05-T03-C06"),
        ("rr1_f09_responses_reasoning_item_never_crosses_providers", "R05-T03-C10"),
        ("rr1_f09_same_family_other_provider_never_receives_the_signature", "R05-T03-C10"),
        ("rr1_f09_same_model_id_different_provider_refuses", "R05-T03-C10"),
        ("rr1_f09_same_origin_replays_the_signature_verbatim", "R05-T03-C10"),
        ("rr1_f09_same_provider_other_model_refuses", "R05-T03-C10"),
        ("rr1_f10_codex_replay_keeps_text_reasoning_tool_order", "R05-T03-C12"),
        ("rr1_f10_responses_keep_reasoning_text_tool_order", "R05-T03-C12"),
        ("rr1_f10_responses_keep_text_reasoning_tool_order", "R05-T03-C12"),
        ("rr1_f10_responses_multi_segment_interleave_keeps_positions", "R05-T03-C12"),
        ("rr1_f10_responses_tools_interspersed_keep_positions", "R05-T03-C12"),
        ("rr1_f32_declared_non_reasoning_model_strips_the_carrier", "R05-T03-C06"),
        ("rr1_f32_no_origin_family_state_refused_on_all_three_gated_families", "R05-T03-C10"),
        ("rr1_f32_off_level_deepseek_round_without_carrier_is_exempt", "R05-T03-C06"),
        ("rr1_f32_off_level_strips_a_present_deepseek_carrier", "R05-T03-C06"),
        ("rr1_f32_responses_duplicated_function_call_anchor_is_refused", "R05-T03-C10"),
        ("rr1_f32_utility_mode_deepseek_round_without_carrier_is_exempt", "R05-T03-C06"),
        ("rr1_f32_utility_mode_strips_a_present_deepseek_carrier", "R05-T03-C06"),
    ],
    "adp:r05_t04_rr1_batch_terminal": [
        ("anthropic_open_tool_block_cannot_close_as_a_completed_batch", "R05-T04-C11"),
        ("buffered_bodies_without_a_normal_stop_reason_are_loud", "R05-T04-C11"),
        ("duplicate_provider_ids_with_conflicting_arguments_reject_whole_batch", "R05-T04-C10"),
        ("identical_resends_collapse_but_distinct_ids_stay_independent", "R05-T04-C10"),
        ("openai_done_without_finish_reason_does_not_make_final", "R05-T04-C11"),
        ("positive_controls_normal_terminals_and_legal_batches_still_work", "R05-T04-C11"),
        ("present_but_unmapped_terminal_values_are_loud_and_name_the_value", "R05-T04-C11"),
        ("thinking_only_and_opaque_only_do_not_form_final_answers", "R05-T04-C11"),
        ("unmapped_terminal_values_through_the_stream_accumulators_stay_loud", "R05-T04-C11"),
    ],
    "adp:r05_t07_rr1_usage_strict": [
        ("rr1_f22_invalid_detail_never_echoes_the_payload", "R05-T07-C09"),
        ("rr1_f22_legal_rerank_usage_still_reports", "R05-T07-C06"),
        ("rr1_f22_minimax_total_tokens_keeps_its_raw_type", "R05-T07-C06"),
        ("rr1_f22_operation_usage_type_table", "R05-T07-C06"),
        ("rr1_f22_rerank_non_numeric_usage_must_not_be_coerced_to_reported_zero", "R05-T07-C06"),
        ("rr1_f23_gemini_component_matrix_and_family_controls", "R05-T07-C07"),
        ("rr1_f23_gemini_thoughts_are_separate_from_candidate_tokens", "R05-T07-C07"),
    ],
}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--inventories", required=True,
                        help="dir with one <run-key>.tests file per suite")
    args = parser.parse_args()
    inv_dir = pathlib.Path(args.inventories)

    suites = ["svc:r05_t01_binary_wiring", "svc:r05_t01_model_plane",
              "svc:r05_t02_credentials", "svc:r05_t03_protocol_adapters",
              "svc:r05_t04_streaming", "svc:r05_t05_network",
              "svc:r05_t05_timeouts", "svc:r05_t06_operations",
              "svc:r05_t06_rr1_media_resource", "svc:r05_t06_rr1_system_speech",
              "svc:r05_t06_worker_model", "svc:r05_t07_persistence",
              "svc:r05_t07_rr1_usage_ledger", "svc:r05_t07_usage_trace",
              "svc:r05_t08_closed_loop", "svc:r05_t08_production_tools",
              "svc:r05_t08_resources",
              "adp:r05_t02_oauth_flows", "adp:r05_t03_goldens",
              "adp:r05_t03_rr1_replay", "adp:r05_t04_rr1_batch_terminal",
              "adp:r05_t04_streaming", "adp:r05_t05_compat",
              "adp:r05_t05_timeouts", "adp:r05_t06_operations",
              "adp:r05_t07_rr1_usage_strict", "adp:r05_t07_usage_families"]
    inventories = {}
    for run in suites:
        path = inv_dir / f"{run.replace(':', '_')}.tests"
        names = [line.strip() for line in path.read_text().splitlines() if line.strip()]
        assert names, f"empty inventory for {run}"
        inventories[run] = names

    # existing lib pins are kept verbatim; suite pins get the fresh counts
    old_pins = [l for l in (R05 / "r05_stage_pins.tsv").read_text().splitlines()
                if l.startswith("pin ")]
    lib_pins = [l for l in old_pins if l.split()[1].startswith("lib-")]
    pin_lines = [f"pin {run} {len(inventories[run])} {run.split(':')[1]}"
                 for run in suites]
    pin_lines += lib_pins

    # ownership: keep the old cid lines whose (run, test) still exists,
    # then add every new mapping, then verify FULL coverage per suite.
    old_cid_lines = [l.split() for l in (R05 / "r05_stage_cids.tsv").read_text().splitlines()
                     if l.startswith("cid ")]
    by_run_names = {run: set(names) for run, names in inventories.items()}
    cid_entries = []  # (cid, run, [names])
    index = {}
    for fields in old_cid_lines:
        _, cid, run, names_field = fields
        if run.startswith("lib-"):
            # lib-pin ownership is kept verbatim (the pin table still
            # registers the run; the assembler resolves the log the same
            # way).
            key = (cid, run)
            if key not in index:
                index[key] = len(cid_entries)
                cid_entries.append([cid, run, []])
            cid_entries[index[key]][2].extend(names_field.split("+"))
            continue
        if run in by_run_names:
            kept = [n for n in names_field.split("+") if n in by_run_names[run]]
            dropped = [n for n in names_field.split("+") if n not in by_run_names[run]]
            if dropped:
                print(f"NOTE: {cid}@{run} dropped moved/renamed tests: {dropped}",
                      file=sys.stderr)
            if kept:
                key = (cid, run)
                if key not in index:
                    index[key] = len(cid_entries)
                    cid_entries.append([cid, run, []])
                cid_entries[index[key]][2].extend(kept)
    for run, mapping in NEW_OWNERSHIP.items():
        for test, cid in mapping:
            assert test in by_run_names[run], f"{run} has no test {test!r}"
            key = (cid, run)
            if key not in index:
                index[key] = len(cid_entries)
                cid_entries.append([cid, run, []])
            assert test not in cid_entries[index[key]][2], f"double-own {test}"
            cid_entries[index[key]][2].append(test)

    # FULL ownership check: every test of every suite owned exactly once.
    errors = []
    seen = {}
    for cid, run, names in cid_entries:
        for n in names:
            if (run, n) in seen:
                errors.append(f"test {n}@{run} claimed by {seen[(run, n)]} and {cid}")
            seen[(run, n)] = cid
    for run, names in inventories.items():
        for n in names:
            if (run, n) not in seen:
                errors.append(f"test {n}@{run} is owned by NO cid")
    if errors:
        print("OWNERSHIP ERRORS:\n" + "\n".join(errors), file=sys.stderr)
        return 1

    # registry cross-checks
    cid_owned = {cid for cid, _, _ in cid_entries}
    for cid in cid_owned:
        assert cid in ALL_REQUIRED, f"cid table owns non-required {cid}"
        assert cid not in COMMAND_BOUND, f"{cid} is both cid-owned and command-bound"
    covered = cid_owned | set(COMMAND_BOUND)
    missing = [c for c in ALL_REQUIRED if c not in covered]
    assert not missing, f"registry entries without binding: {missing}"
    assert len(cid_owned) + len(COMMAND_BOUND) == 103

    cid_lines = [f"cid {cid} {run} {'+'.join(sorted(names))}"
                 for cid, run, names in sorted(cid_entries)]
    registry_lines = [
        "# R05 RR1 F26: the AUTHORITATIVE required-C-ID registry (103 = the",
        "# 100 original C-IDs of the 2026-10-02 special prompt (indexed in",
        "# RR1_MASTER_PROMPT_2026-10-04.md 附录B) + the 3 appended C-IDs).",
        "# binding: `cid` = owned by r05_stage_cids.tsv (exact set equality",
        "# enforced by the xtask mirror AND the stage-suites assembler);",
        "# `command:<key>` = bound to a registered R05 stage-map command",
        "# (the shared execution is valid evidence; the named producer tests",
        "# of each binding are documented in R05_TEST_MAP.json).",
    ]
    for cid in ALL_REQUIRED:
        binding = f"command:{COMMAND_BOUND[cid]}" if cid in COMMAND_BOUND else "cid"
        registry_lines.append(f"reqcid {cid} {binding}")

    (R05 / "r05_stage_pins.tsv").write_text("\n".join(sorted(pin_lines)) + "\n")
    (R05 / "r05_stage_cids.tsv").write_text("\n".join(cid_lines) + "\n")
    (R05 / "r05_required_cids.tsv").write_text("\n".join(registry_lines) + "\n")
    print(f"pins: {len(suites)} suites + {len(lib_pins)} lib pins; "
          f"cid-owned: {len(cid_owned)}; command-bound: {len(COMMAND_BOUND)}; "
          f"total registry: {len(cid_owned) + len(COMMAND_BOUND)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
