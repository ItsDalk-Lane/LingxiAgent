//! 显式维护、启动及摄入循环每小时维护共用实际策略。
use super::*;

const DAY_MS: i64 = 86_400_000;
const ORPHAN_RETENTION_DAYS: i64 = 7;

impl KnowledgeService {
    pub(super) fn prune_orphan_indexes(&self, source: &str) -> Result<bool, KnowledgeError> {
        self.facts
            .prune_orphan_indexes(&self.studio_id, source, |artifacts| {
                for (artifact, _) in artifacts {
                    self.vectors.remove_artifact(artifact)?;
                    self.index.remove_artifact(artifact)?;
                }
                Ok(())
            })
    }
    /// 默认永久保留向量；有限期限只清旧版本。孤儿资料七天后仍需逐项检查引用。
    pub fn run_lifecycle_maintenance(&self) -> Result<Value, KnowledgeError> {
        let _write = self.write_gate.try_lock().map_err(|_| {
            KnowledgeError::storage_invalid("knowledge write gate busy; lifecycle not run")
        })?;
        let now = (self.clock)();
        let usage = self.vectors.list_usage()?;
        let mut latest = std::collections::HashMap::<String, i64>::new();
        for entry in &usage {
            if entry.status == "ready" {
                latest
                    .entry(entry.artifact_id.clone())
                    .and_modify(|v| *v = (*v).max(entry.indexed_at_ms))
                    .or_insert(entry.indexed_at_ms);
            }
        }
        let mut removed = Vec::new();
        for entry in &usage {
            let did_remove = self.facts.with_artifact_retention(
                &self.studio_id,
                &entry.artifact_id,
                |owner| {
                    let Some(owner) = owner else {
                        return Ok(false);
                    };
                    let Some(days) = owner.retention_days else {
                        return Ok(false);
                    };
                    if days <= 0 {
                        return Err(KnowledgeError::storage_invalid(
                            "invalid stored vector retention days; cache preserved",
                        ));
                    }
                    if owner.protected
                        || (owner.latest
                            && latest.get(&entry.artifact_id) == Some(&entry.indexed_at_ms))
                    {
                        return Ok(false);
                    }
                    let cutoff = now.saturating_sub(days.saturating_mul(DAY_MS));
                    self.vectors
                        .remove_unused_variant(&entry.variant_id, cutoff)
                },
            )?;
            if did_remove {
                removed.push(entry.variant_id.clone());
            }
        }
        let cutoff = now.saturating_sub(ORPHAN_RETENTION_DAYS * DAY_MS);
        let mut purged = Vec::new();
        let mut skipped = Vec::new();
        for source in self.facts.orphan_candidates(&self.studio_id, cutoff)? {
            let result = self.facts.purge_orphan_if_unreferenced(
                &self.studio_id,
                &source,
                cutoff,
                |artifacts| {
                    for (artifact, _) in artifacts {
                        self.vectors.remove_artifact(artifact)?;
                        self.index.remove_artifact(artifact)?;
                    }
                    Ok(())
                },
            )?;
            match result {
                None => purged.push(source),
                Some(reason) => skipped.push(json!({"sourceId":source,"reason":reason})),
            }
        }
        Ok(
            json!({"removedVectorVariantIds":removed,"purgedSourceIds":purged,"skippedSources":skipped,"orphanRetentionDays":ORPHAN_RETENTION_DAYS,"maintenanceTrigger":"explicit_or_ingestion_worker"}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI64, Ordering};

    fn fixture(tag: &str) -> (std::path::PathBuf, Arc<AtomicI64>, KnowledgeService) {
        let home = std::env::temp_dir().join(ks::new_id(tag));
        std::fs::create_dir_all(&home).unwrap();
        let now = Arc::new(AtomicI64::new(1_790_000_000_000));
        let tick = now.clone();
        let service = KnowledgeService::new_with_clock(
            &home,
            "retention-studio".into(),
            Arc::new(move || tick.load(Ordering::SeqCst)),
        )
        .unwrap();
        (home, now, service)
    }
    fn notebook(s: &KnowledgeService, days: Option<i64>) -> String {
        let id = s.create_notebook("保留期合成资料").unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        s.update_notebook_settings(&id, &json!({"vectorRetentionDays":days}))
            .unwrap();
        id
    }
    fn source(s: &KnowledgeService, book: &str) -> (ks::SourceRow, ks::SnapshotRow, String) {
        let (source, snapshot) = s
            .facts
            .create_source_with_snapshot(
                s.studio_id(),
                "pasted_text",
                "原文",
                &json!({}),
                b"original bytes preserved",
                "text/plain",
            )
            .unwrap();
        s.facts.add_membership(book, &source.id).unwrap();
        let artifact = ks::new_id("artifact");
        s.facts
            .begin_parse_artifact(&artifact, &snapshot.id, "fixture", "1", "config")
            .unwrap();
        s.facts
            .complete_parse_artifact(&artifact, "ready", &[], "citation_grade", None)
            .unwrap();
        (source, snapshot, artifact)
    }
    fn variant(s: &KnowledgeService, artifact: &str, key: &str) -> String {
        let build = s
            .vectors
            .begin_variant_build(
                &format!("fixture-civ-{artifact}"),
                artifact,
                key,
                "exact-content",
                2,
            )
            .unwrap();
        s.vectors
            .upsert_batch(
                &build.variant_id,
                &[ks::VectorEntry {
                    chunk_id: format!("chunk-{key}"),
                    ordinal: 0,
                    vector: vec![0.5, 0.75],
                }],
            )
            .unwrap();
        s.vectors.complete_build(&build.variant_id, 1).unwrap();
        build.variant_id
    }
    fn exists(s: &KnowledgeService, id: &str) -> bool {
        s.vectors.get_variant(id).unwrap().is_some()
    }

    #[test]
    fn fixed_clock_old_vectors_expire_current_and_original_bytes_survive_restart() {
        let (home, now, s) = fixture("retention");
        let book = notebook(&s, Some(1));
        let (source, snapshot, artifact) = source(&s, &book);
        let old = variant(&s, &artifact, "old");
        now.fetch_add(10, Ordering::SeqCst);
        let current = variant(&s, &artifact, "current");
        let original = s.vectors.checkpoint_entries(&current).unwrap();
        now.fetch_add(2 * DAY_MS, Ordering::SeqCst);
        let report = s.run_lifecycle_maintenance().unwrap();
        assert_eq!(report["removedVectorVariantIds"], json!([old]));
        assert!(!exists(&s, &old));
        assert!(exists(&s, &current));
        assert_eq!(s.vectors.checkpoint_entries(&current).unwrap(), original);
        assert_eq!(
            s.facts.read_snapshot_bytes(&snapshot).unwrap(),
            b"original bytes preserved"
        );
        assert!(s.facts.get_source(&source.id).is_ok());
        drop(s);
        let tick = now.clone();
        let s = KnowledgeService::new_with_clock(
            &home,
            "retention-studio".into(),
            Arc::new(move || tick.load(Ordering::SeqCst)),
        )
        .unwrap();
        assert!(!exists(&s, &old));
        assert!(exists(&s, &current));
        assert_eq!(s.vectors.checkpoint_entries(&current).unwrap(), original);
        assert!(
            s.run_lifecycle_maintenance().unwrap()["removedVectorVariantIds"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn actual_query_refreshes_usage_persistently_and_old_scan_cannot_delete_it() {
        let (home, now, s) = fixture("retention-query");
        let book = notebook(&s, Some(1));
        let (_, _, artifact) = source(&s, &book);
        let old = variant(&s, &artifact, "old");
        now.fetch_add(10, Ordering::SeqCst);
        let current = variant(&s, &artifact, "current");
        now.fetch_add(3 * DAY_MS, Ordering::SeqCst);
        let cutoff = now.load(Ordering::SeqCst) - DAY_MS;
        assert_eq!(
            s.vectors
                .search(std::slice::from_ref(&old), &[0.5, 0.75], 5)
                .unwrap()
                .len(),
            1
        );
        assert!(!s.vectors.remove_unused_variant(&old, cutoff).unwrap());
        s.run_lifecycle_maintenance().unwrap();
        assert!(exists(&s, &old));
        let usage = s.vectors.list_usage().unwrap();
        drop(s);
        let tick = now.clone();
        let s = KnowledgeService::new_with_clock(
            &home,
            "retention-studio".into(),
            Arc::new(move || tick.load(Ordering::SeqCst)),
        )
        .unwrap();
        assert_eq!(
            s.vectors
                .list_usage()
                .unwrap()
                .iter()
                .map(|v| (&v.variant_id, v.last_used_at_ms))
                .collect::<Vec<_>>(),
            usage
                .iter()
                .map(|v| (&v.variant_id, v.last_used_at_ms))
                .collect::<Vec<_>>()
        );
        now.fetch_add(DAY_MS - 1, Ordering::SeqCst);
        s.run_lifecycle_maintenance().unwrap();
        assert!(exists(&s, &old));
        now.fetch_add(1, Ordering::SeqCst);
        s.run_lifecycle_maintenance().unwrap();
        assert!(!exists(&s, &old));
        assert!(exists(&s, &current));
    }

    #[test]
    fn shared_null_means_forever_maximum_days_wins_and_foreign_maintenance_is_inert() {
        let (home, now, s) = fixture("retention-shared");
        let a = notebook(&s, Some(1));
        let b = notebook(&s, None);
        let (source, _, artifact) = source(&s, &a);
        s.facts.add_membership(&b, &source.id).unwrap();
        let old = variant(&s, &artifact, "old");
        now.fetch_add(10, Ordering::SeqCst);
        variant(&s, &artifact, "current");
        now.fetch_add(2 * DAY_MS, Ordering::SeqCst);
        s.run_lifecycle_maintenance().unwrap();
        assert!(exists(&s, &old));
        s.update_notebook_settings(&b, &json!({"vectorRetentionDays":3}))
            .unwrap();
        s.run_lifecycle_maintenance().unwrap();
        assert!(exists(&s, &old));
        now.fetch_add(2 * DAY_MS, Ordering::SeqCst);
        let tick = now.clone();
        let foreign = KnowledgeService::new_with_clock(
            &home,
            "other-studio".into(),
            Arc::new(move || tick.load(Ordering::SeqCst)),
        )
        .unwrap();
        assert!(
            foreign.run_lifecycle_maintenance().unwrap()["removedVectorVariantIds"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(exists(&s, &old));
        s.run_lifecycle_maintenance().unwrap();
        assert!(!exists(&s, &old));
    }

    #[test]
    fn notebook_delete_retains_seven_days_then_purges_only_unreferenced_owned_material() {
        let (home, now, s) = fixture("orphan-seven");
        let book = notebook(&s, None);
        let (source, snapshot, _) = source(&s, &book);
        let original = home.join("original-user-file.txt");
        std::fs::write(&original, b"user file remains").unwrap();
        let result = s.delete_notebook(&book).unwrap();
        assert_eq!(result["deletedSourceIds"], json!([]));
        assert_eq!(result["orphanedSourceIds"], json!([source.id]));
        assert_eq!(result["sourceRetentionDays"], 7);
        assert!(s
            .facts
            .get_source(&source.id)
            .unwrap()
            .orphaned_at
            .is_some());
        assert!(s.facts.get_notebook(s.studio_id(), &book).is_err());
        now.fetch_add(7 * DAY_MS - 1, Ordering::SeqCst);
        s.run_lifecycle_maintenance().unwrap();
        assert!(s.facts.get_source(&source.id).is_ok());
        assert!(home.join("knowledge").join(&snapshot.storage_path).exists());
        drop(s);
        let tick = now.clone();
        let s = KnowledgeService::new_with_clock(
            &home,
            "retention-studio".into(),
            Arc::new(move || tick.load(Ordering::SeqCst)),
        )
        .unwrap();
        now.fetch_add(1, Ordering::SeqCst);
        assert_eq!(
            s.run_lifecycle_maintenance().unwrap()["purgedSourceIds"],
            json!([source.id])
        );
        assert!(s.facts.get_source(&source.id).is_err());
        assert!(!home.join("knowledge").join(&snapshot.storage_path).exists());
        assert_eq!(std::fs::read(original).unwrap(), b"user file remains");
        assert!(s.run_lifecycle_maintenance().unwrap()["purgedSourceIds"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn orphan_reattach_clears_timer_and_scope_jobs_citations_protect_actual_bytes() {
        let (home, now, s) = fixture("orphan-protected");
        let book = notebook(&s, None);
        let keeper = notebook(&s, None);
        let (revived, _, _) = source(&s, &book);
        s.remove_source_from_notebook(&book, &revived.id).unwrap();
        s.facts.add_membership(&keeper, &revived.id).unwrap();
        assert!(s
            .facts
            .get_source(&revived.id)
            .unwrap()
            .orphaned_at
            .is_none());
        let (scoped, scope_snapshot, _) = source(&s, &book);
        s.create_turn_scope("owner", "turn", std::slice::from_ref(&book))
            .unwrap();
        s.remove_source_from_notebook(&book, &scoped.id).unwrap();
        let (busy, busy_snapshot, artifact) = source(&s, &book);
        s.facts
            .enqueue_source_ingestion(&book, &busy.id, Some(&artifact), "profile")
            .unwrap();
        s.remove_source_from_notebook(&book, &busy.id).unwrap();
        let (cited, cited_snapshot, artifact) = source(&s, &book);
        let block = ks::BlockRow {
            id: ks::new_id("block"),
            parse_artifact_id: artifact.clone(),
            ordinal: 0,
            text: "证据原文".into(),
            text_sha256: ks::sha256_hex("证据原文".as_bytes()),
            locator_type: "paragraph".into(),
            locator: json!({"ordinal":0}),
        };
        s.facts.insert_blocks(std::slice::from_ref(&block)).unwrap();
        let citation = s.facts.create_citation(&artifact, &block.id, 0, 4).unwrap();
        s.remove_source_from_notebook(&book, &cited.id).unwrap();
        now.fetch_add(8 * DAY_MS, Ordering::SeqCst);
        let result = s.run_lifecycle_maintenance().unwrap();
        assert!(result["purgedSourceIds"].as_array().unwrap().is_empty());
        let reasons = result["skippedSources"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["reason"].as_str().unwrap())
            .collect::<std::collections::HashSet<_>>();
        assert!(reasons.contains("active-turn-scope-reference"));
        assert!(reasons.contains("active-ingestion-job"));
        assert!(reasons.contains("persisted-citation-reference"));
        for snapshot in [&scope_snapshot, &busy_snapshot, &cited_snapshot] {
            assert_eq!(
                std::fs::read(home.join("knowledge").join(&snapshot.storage_path)).unwrap(),
                b"original bytes preserved"
            );
        }
        assert_eq!(
            s.facts
                .resolve_citation(&citation.id)
                .unwrap()
                .citation
                .canonical_text,
            "证据原文"
        );
        assert!(s
            .facts
            .add_membership(&keeper, &ks::new_id("missing"))
            .is_err());
    }

    #[test]
    fn legacy_usage_backfill_preserves_exact_vector_bytes_and_future_schema_is_not_written() {
        let root = std::env::temp_dir().join(ks::new_id("vector-usage-migration"));
        let old = ks::KnowledgeVectorStore::open_with_clock(&root, Arc::new(|| 1000)).unwrap();
        let build = old
            .begin_variant_build("civ", "artifact", "model", "fingerprint", 2)
            .unwrap();
        old.upsert_batch(
            &build.variant_id,
            &[ks::VectorEntry {
                chunk_id: "chunk".into(),
                ordinal: 0,
                vector: vec![0.25, -0.75],
            }],
        )
        .unwrap();
        old.complete_build(&build.variant_id, 1).unwrap();
        let bytes = old.checkpoint_entries(&build.variant_id).unwrap();
        drop(old);
        let db = root.join("knowledge_vector.db");
        {
            let conn = rusqlite::Connection::open(&db).unwrap();
            conn.execute_batch("DROP TABLE knowledge_vector_usage")
                .unwrap();
        }
        let migrated = ks::KnowledgeVectorStore::open_with_clock(&root, Arc::new(|| 9000)).unwrap();
        assert_eq!(
            migrated.checkpoint_entries(&build.variant_id).unwrap(),
            bytes
        );
        let usage = migrated.list_usage().unwrap();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].indexed_at_ms, 9000);
        assert_eq!(usage[0].last_used_at_ms, 9000);
        assert!(!migrated
            .remove_unused_variant(&build.variant_id, 8999)
            .unwrap());
        drop(migrated);
        {
            let conn = rusqlite::Connection::open(&db).unwrap();
            conn.pragma_update(None, "user_version", 2).unwrap();
        }
        let before = std::fs::read(&db).unwrap();
        assert!(ks::KnowledgeVectorStore::open(&root).is_err());
        assert_eq!(std::fs::read(&db).unwrap(), before);
    }

    #[test]
    fn failed_job_with_unsettled_paid_request_keeps_checkpoint_and_source_past_retention() {
        let (home, now, s) = fixture("orphan-unsettled-paid");
        let book = notebook(&s, Some(1));
        let (source, snapshot, artifact) = source(&s, &book);
        let variant = variant(&s, &artifact, "paid-checkpoint");
        let original = s.vectors.checkpoint_entries(&variant).unwrap();
        let job = s
            .facts
            .enqueue_source_ingestion(&book, &source.id, Some(&artifact), "profile")
            .unwrap();
        s.facts.claim_next_job().unwrap().unwrap();
        s.facts
            .set_job_embedding_stats(
                &job.id,
                &json!({"unknownSentRequestCount":1,"checkpointVariantId":variant}),
            )
            .unwrap();
        s.facts.fail_job(&job.id, "unknown remote outcome").unwrap();
        s.remove_source_from_notebook(&book, &source.id).unwrap();
        now.fetch_add(8 * DAY_MS, Ordering::SeqCst);
        let result = s.run_lifecycle_maintenance().unwrap();
        assert_eq!(
            result["skippedSources"][0]["reason"],
            "unsettled-paid-request"
        );
        assert!(result["purgedSourceIds"].as_array().unwrap().is_empty());
        assert_eq!(s.vectors.checkpoint_entries(&variant).unwrap(), original);
        assert_eq!(
            std::fs::read(home.join("knowledge").join(&snapshot.storage_path)).unwrap(),
            b"original bytes preserved"
        );
    }

    #[cfg(unix)]
    #[test]
    fn actual_gc_refuses_replaced_artifact_directory_before_removing_original_or_outside_file() {
        use std::os::unix::fs::symlink;
        let (home, now, s) = fixture("gc-link-negative");
        let book = notebook(&s, None);
        let (source, snapshot, _) = source(&s, &book);
        s.delete_notebook(&book).unwrap();
        let outside = home.join("outside-original-material");
        std::fs::create_dir_all(outside.join(&snapshot.id)).unwrap();
        std::fs::write(
            outside.join(&snapshot.id).join("keep.json"),
            b"OUTSIDE-MUST-STAY",
        )
        .unwrap();
        let artifacts = home.join("knowledge/artifacts");
        if artifacts.exists() {
            std::fs::remove_dir_all(&artifacts).unwrap();
        }
        symlink(&outside, &artifacts).unwrap();
        now.fetch_add(8 * DAY_MS, Ordering::SeqCst);
        assert!(s.run_lifecycle_maintenance().is_err());
        assert_eq!(
            std::fs::read(outside.join(&snapshot.id).join("keep.json")).unwrap(),
            b"OUTSIDE-MUST-STAY"
        );
        assert_eq!(
            s.facts.read_snapshot_bytes(&snapshot).unwrap(),
            b"original bytes preserved"
        );
        assert!(s.facts.get_source(&source.id).is_ok());
        std::fs::remove_file(artifacts).unwrap();
        assert_eq!(
            s.run_lifecycle_maintenance().unwrap()["purgedSourceIds"],
            json!([source.id])
        );
    }
}
