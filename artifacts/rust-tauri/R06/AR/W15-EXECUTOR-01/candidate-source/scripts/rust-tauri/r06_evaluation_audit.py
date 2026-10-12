#!/usr/bin/env python3
"""核对真实评测证据与未完成范围；通过只表示记录诚实，不代表阶段验收。"""
import argparse
import hashlib
import json
from pathlib import Path


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"重复字段：{key}")
        result[key] = value
    return result


def read_json(path):
    return json.loads(Path(path).read_text(), object_pairs_hook=unique_object)


def digest(value):
    return hashlib.sha256(json.dumps(value, ensure_ascii=False, sort_keys=True).encode()).hexdigest()


def require(condition, label):
    if not condition:
        raise ValueError(label)


def audit_metrics(records, facts):
    indexed = {record["taskId"]: record for record in records}
    required = {f"BE-{number:02}" for number in range(1, 11)}
    require(len(indexed) == len(records) and set(indexed) == required,
            f"固定行为电池或负控缺失：{sorted(required-set(indexed))}")
    raw_rows = {}
    for fact in facts:
        if fact["kind"] == "usage_rows":
            raw_rows[(fact["value"]["databaseHome"], fact["value"]["runId"])] = fact["value"]["rows"]
    for task_id, record in indexed.items():
        score = record["score"]
        dimensions = [score[key] for key in ("toolSelection", "schemaValid", "semanticParamsCorrect", "taskComplete")]
        require(all(type(value) is bool for value in dimensions), f"{task_id} 四维须独立布尔值")
        require(score["overall"] == ("pass" if all(dimensions) else "fail"), f"{task_id} 四维混算")
        require(record["extraModelCalls"] == max(0, record["modelCallsObserved"]-record["modelCallsExpected"]), f"{task_id} 调用次数计算错误")
        require(record.get("liveProviderExecuted") is False and record.get("incumbentEngineExecuted") is False,
                f"{task_id} 固定替身不能冒充LIVE或旧引擎")
        tokens = record["tokens"]
        if task_id in ("BE-06", "BE-08"):
            require(tokens.get("inputTokens") is None and tokens.get("outputTokens") is None
                    and tokens.get("measurementStatus") == "NOT_MEASURED", f"{task_id} 未测量冒零")
            continue
        rows = tokens["ledgerRows"]
        expected_rows = []
        for home, run_id in dict.fromkeys((row["databaseHome"], row["runId"]) for row in rows):
            require((home, run_id) in raw_rows, f"{task_id} run {run_id} 缺少原始SQL记录")
            expected_rows.extend(raw_rows[(home, run_id)])
            sql = [fact["value"] for fact in facts if fact["kind"] == "usage_sql_readback"
                   and fact["value"]["databaseHome"] == home and fact["value"]["runId"] == run_id]
            require(bool(sql) and all(item["fixtureKind"] == "actual production ledger" and item["rows"] == sql[-1]["rows"] for item in sql), f"{task_id} 缺少同窗完整生产SQL")
            transformed = [{"databaseHome": home, "runId": item["run_id"], "purpose": item["purpose"],
                            "outcome": item["outcome"], "inputTokens": item["input_tokens"], "outputTokens": item["output_tokens"]}
                           for item in sql[-1]["rows"]]
            require(transformed == raw_rows[(home, run_id)], f"{task_id} 简表与完整SQL不符")
        require(rows == expected_rows, f"{task_id} 账本行与原始SQL不一致")
        homes = {row["databaseHome"] for row in rows}
        endpoints = {fact["value"]["providerEndpoint"] for fact in facts if fact["kind"] == "run_accepted"
                     and fact["value"]["databaseHome"] in homes}
        physical = [fact for fact in facts if fact["kind"] == "provider_exchange" and fact["value"]["endpoint"] in endpoints]
        require(len(physical) == record["modelCallsObserved"], f"{task_id} 调用次数缺少物理线体支持")
        for key, coverage_key in (("inputTokens", "inputCoverage"), ("outputTokens", "outputCoverage")):
            known = [row[key] for row in rows if row[key] is not None]
            total = sum(known) if rows and len(known) == len(rows) else None
            coverage = tokens[coverage_key]
            require(tokens[key] == total and coverage["total"] == total, f"{task_id} {key} 未测量冒零或求和错误")
            require(coverage["knownRows"] == len(known) and coverage["missingRows"] == len(rows)-len(known)
                    and coverage["knownSubtotal"] == sum(known), f"{task_id} {key} 覆盖率与事实不符")
        for outcome in set(row["outcome"] for row in rows):
            part = [row for row in rows if row["outcome"] == outcome]
            group = tokens["byLedgerOutcome"][outcome]
            require(group["rowCount"] == len(part), f"{task_id} outcome 行数混算")
            for key, name in (("inputTokens", "input"), ("outputTokens", "output")):
                known = [row[key] for row in part if row[key] is not None]
                total = sum(known) if len(known) == len(part) else None
                require(group[name]["total"] == total and group[name]["knownSubtotal"] == sum(known),
                        f"{task_id} outcome {outcome} 用量混算")
        succeeded = sum(row["purpose"] == "chat" and row["outcome"] == "succeeded" for row in rows)
        require(tokens["ledgerChatSucceededRows"] == succeeded, f"{task_id} 成功调用口径混淆")
    negative = indexed["BE-10"]
    require(negative["score"] == {"toolSelection": True, "schemaValid": True, "semanticParamsCorrect": False,
                                 "taskComplete": False, "overall": "fail"}, "R06-A15 错误绑定：须保留BE10语义失败负控")
    require(negative["observedBehavior"]["targetFileAUnchanged"] is True and negative["observedBehavior"]["wrongFileBWritten"] is True,
            "R06-A15 缺少实际目标文件正反事实")
    finals = [fact["value"] for fact in facts if fact["kind"] == "world_final"]
    be10 = next((world for world in finals if "be10-ws" in world["workspace"]), None)
    require(be10 is not None, "R06-A15 缺少真实文件结果")
    listing = dict(be10["workspaceListing"])
    require(listing.get("a.md") == hashlib.sha256(b"A-ORIGINAL").hexdigest()
            and listing.get("b.md") == hashlib.sha256(b"DONE-42").hexdigest(), "R06-A15 实际文件效果与评分不符")
    be04 = indexed["BE-04"]
    require(len(be04["tokens"]["ledgerRows"]) == be04["modelCallsObserved"] == 3, "BE04 三腿物理调用与账本未绑定")
    observation = be04["observedBehavior"]
    require(observation["explainLeg"]["runStatus"] == "completed"
            and observation["forceLegNormalPrompt"]["runStatus"] == observation["forceLegInjectionPrompt"]["runStatus"] == "failed"
            and observation["failedRunCount"] == 2, "R06-A16 失败任务与模型调用成功混算")
    for leg in ("explainLeg", "forceLegNormalPrompt", "forceLegInjectionPrompt"):
        require(observation[leg]["invocations"] == 0, f"R06-A16 {leg} 目录外工具派发")
    # 三腿按资料根、run、会话和loopback地址绑定，独立服务重号不能串证。
    for marker, leg in (("BE04A", "explainLeg"), ("BE04B", "forceLegNormalPrompt"), ("BE04C", "forceLegInjectionPrompt")):
        run_id = observation[leg]["runId"]
        accept = [fact["value"] for fact in facts if fact["kind"] == "run_accepted" and marker in fact["value"]["input"]]
        require(len(accept) == 1 and accept[0]["runId"] == run_id, f"BE04 {marker} 缺少实际接纳绑定")
        accepted = accept[0]
        wires = [fact["value"] for fact in facts if fact["kind"] == "provider_exchange"
                 and fact["value"]["endpoint"] == accepted["providerEndpoint"]
                 and marker in json.dumps(fact["value"]["request"], ensure_ascii=False)]
        require(len(wires) == 1, f"BE04 {marker} 物理调用不唯一")
        usages = [json.loads(line[6:])["usage"] for line in wires[0]["responseSse"].splitlines()
                  if line.startswith("data: {") and "usage" in json.loads(line[6:])]
        ledger = [row for row in be04["tokens"]["ledgerRows"] if row["runId"] == run_id and row["databaseHome"] == accepted["databaseHome"]]
        require(len(usages) == len(ledger) == 1 and usages[0]["prompt_tokens"] == ledger[0]["inputTokens"]
                and usages[0]["completion_tokens"] == ledger[0]["outputTokens"], f"BE04 {marker} 线体和账本实测不符")
    media = [fact["value"] for fact in facts if fact["kind"] == "media_wire_and_products"]
    require(len(media) == 1 and media[0]["physicalCalls"] == indexed["BE-06"]["modelCallsObserved"] == 2
            and len(media[0]["productsA"]) == len(media[0]["productsB"]) == 1, "BE06 媒体调用或落盘缺实证")
    child = [fact["value"] for fact in facts if fact["kind"] == "subagent_boundary"]
    require(len(child) == 1 and len(child[0]["capturedTurns"]) == indexed["BE-08"]["modelCallsObserved"]
            and child[0]["dispatched"] == 0 and not child[0]["executorArrivals"]
            and not child[0]["workspaceListing"], "BE08 越权拒绝缺实际边界证据")
    require("OPEN_F07" in indexed["BE-02"].get("originalRequirementStatus", ""),
            "BE02 常驻目录稳定性不能绑定按需发现描述调用完整业务")
    controls = {fact["value"]["marker"]: fact["value"] for fact in facts if fact["kind"] == "wire_coverage_evaluation"}
    require(set(controls) == {"MEASURED-ZERO", "UNKNOWN", "PARTIAL"}, "真实线体缺测/实零/部分缺测控制缺失")
    for marker, control in controls.items():
        metrics = control["metrics"]
        require(len(metrics["ledgerRows"]) == 1, f"{marker} 须有单次实际SQL行")
        row = metrics["ledgerRows"][0]
        accepted = [fact["value"] for fact in facts if fact["kind"] == "run_accepted"
                    and fact["value"]["databaseHome"] == row["databaseHome"] and fact["value"]["runId"] == row["runId"]]
        require(len(accepted) == 1 and accepted[0]["input"] == marker, f"{marker} 资料根/run绑定错误")
        wires = [fact["value"] for fact in facts if fact["kind"] == "provider_exchange"
                 and fact["value"]["endpoint"] == accepted[0]["providerEndpoint"]
                 and any(message.get("role") == "user" and message.get("content") == marker
                         for message in fact["value"]["request"]["messages"])]
        require(len(wires) == 1, f"{marker} 缺少唯一物理响应")
        usage_frames = [json.loads(line[6:])["usage"] for line in wires[0]["responseSse"].splitlines()
                        if line.startswith("data: {") and "usage" in json.loads(line[6:])]
        require(len(usage_frames) <= 1, f"{marker} 本夹具出现多用量帧")
        usage = usage_frames[0] if usage_frames else {}
        require(raw_rows[(row["databaseHome"], row["runId"])] == [row], f"{marker} SQL与评测不符")
        for field, wire_key, coverage_key in (("inputTokens", "prompt_tokens", "inputCoverage"),
                                               ("outputTokens", "completion_tokens", "outputCoverage")):
            observed = usage.get(wire_key)
            require(row[field] == metrics[field] == observed, f"{marker} {field} 缺测冒零/实测漂移")
            coverage = metrics[coverage_key]
            require(coverage["knownRows"] == int(observed is not None)
                    and coverage["missingRows"] == int(observed is None)
                    and coverage["measurementStatus"] == ("MEASURED" if observed is not None else "NOT_MEASURED"),
                    f"{marker} {field} 覆盖率与真实响应不符")
    return indexed


def audit_scope(original, report, records, root):
    require(report["original_obligations_digest"] == digest(original), "原义务绑定漂移")
    obligations = {row["id"]: row for row in original["obligations"]}
    current = {row["id"]: row for row in report["obligations"]}
    require(len(current) == len(report["obligations"]) and set(current) == set(obligations), "原1234义务有删减或重复")
    for identifier, row in current.items():
        expected = obligations[identifier]
        require(row["original_clause_digest"] == digest(expected["original_clause"]), f"{identifier} 原断言被改松")
        require(row["status"] == "OPEN_NOT_CLOSED_BY_W15", f"{identifier} source-only/机制echo不得关闭完整业务；本Task没有该叶的独立语义验收")
        require(row["deadline"] == expected["deadline"], f"{identifier} 无原依据后移期限")
    leaves = {row["id"]: row for row in report["leaves"]}
    original_leaves = {row["id"]: row for row in original["r00_scope_dispositions"]}
    require(set(leaves) == set(original_leaves), "原281叶缺失或新增未声明")
    for identifier, row in leaves.items():
        require(row["status"] == "OPEN_NOT_CLOSED_BY_W15", f"{identifier} missing→deferred或伪造业务通过；原R06义务仍须履行")
        require(row["original_disposition_digest"] == digest(original_leaves[identifier]), f"{identifier} 原份额/后续依据被换写")
    require(report["acceptance_bindings"]["R06-A15"] == ["BE-10"]
            and report["acceptance_bindings"]["R06-A16"] == ["BE-04", "BE-05"], "A15/A16 被echo/握手错误绑定")
    require(report["current_stage_status"] == "R06_NOT_ACCEPTED" and report["independent_review"] == "NOT_RUN"
            and report["stage_gate"] == "NOT_RUN" and report["git_delivery"] == "NOT_COMMITTED_NOT_PUSHED",
            "当前放行状态与未决集合/真实Git事实矛盾")
    for path, expected in report["candidate_sources"].items():
        require(hashlib.sha256((root/path).read_bytes()).hexdigest() == expected, f"候选漂移/旧证据复用：{path}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--records", required=True)
    parser.add_argument("--facts", required=True)
    parser.add_argument("--obligations")
    parser.add_argument("--scope")
    parser.add_argument("--root", default=".")
    args = parser.parse_args()
    try:
        records = [json.loads(line, object_pairs_hook=unique_object) for line in Path(args.records).read_text().splitlines()]
        facts = [json.loads(line, object_pairs_hook=unique_object) for line in Path(args.facts).read_text().splitlines()]
        indexed = audit_metrics(records, facts)
        if args.scope or args.obligations:
            require(args.scope and args.obligations, "范围检查需同时提供原义务与当前报告")
            audit_scope(read_json(args.obligations), read_json(args.scope), indexed, Path(args.root))
        print("记录与真实证据一致；未决义务保留。此结果不是R06阶段PASS。")
        return 0
    except (ValueError, KeyError, OSError, TypeError, IndexError) as error:
        print(f"拒绝虚报或缺测：{error}")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
