use lingxi_adapters::storage::knowledge::KnowledgeIndexStore;
use lingxi_test_guard as _;
use rusqlite::Connection;
fn main() {
    let home=std::path::PathBuf::from(std::env::var_os("LINGXI_HOME").unwrap());
    let root=home.join("counter-old-status");
    let old=KnowledgeIndexStore::open(&root).unwrap();
    let variant=old.ensure_chunk_index_variant("artifact", "profile", "fingerprint").unwrap();
    let newer=Connection::open(root.join("knowledge_index.db")).unwrap();
    newer.execute_batch("PRAGMA user_version=999").unwrap();
    let before:String=newer.query_row("SELECT status FROM chunk_index_variants WHERE id=?1",[&variant.id],|r|r.get(0)).unwrap();
    let result=old.set_variant_status(&variant.id,"failed");
    let after:String=newer.query_row("SELECT status FROM chunk_index_variants WHERE id=?1",[&variant.id],|r|r.get(0)).unwrap();
    let version:i64=newer.query_row("PRAGMA user_version",[],|r|r.get(0)).unwrap();
    println!("home={} result={:?} before={} after={} schema={}",home.display(),result,before,after,version);
    assert!(matches!(result,Err(ref e) if e.code=="KNOWLEDGE_SCHEMA_NEWER"),"旧句柄必须拒绝写入future库");
    assert_eq!(before,after,"拒绝后状态必须不变");
}
