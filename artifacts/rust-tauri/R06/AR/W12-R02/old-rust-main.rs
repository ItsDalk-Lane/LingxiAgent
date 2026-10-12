use lingxi_test_guard as _;
use rusqlite::Connection;
fn main(){
 let home=std::path::PathBuf::from(std::env::var_os("LINGXI_HOME").unwrap());
 let mut preserved=true;
 for name in ["knowledge_index.db","knowledge_vector.db"] {
  let root=home.join(name);std::fs::create_dir_all(&root).unwrap();let path=root.join(name);
  let writer=Connection::open(&path).unwrap();writer.execute_batch("CREATE TABLE sentinel(value TEXT);INSERT INTO sentinel VALUES('paid-original');PRAGMA user_version=999;").unwrap();drop(writer);
  let before=std::fs::read(&path).unwrap();
  let result=if name=="knowledge_index.db"{old::KnowledgeIndexStore::open(&root).map(|_|())}else{old::KnowledgeVectorStore::open(&root).map(|_|())};
  let probe=Connection::open(&path).unwrap();
  let version:i64=probe.query_row("PRAGMA user_version",[],|r|r.get(0)).unwrap();
  let sentinel=probe.query_row("SELECT value FROM sentinel",[],|r|r.get::<_,String>(0));
  let bytes_same=std::fs::read(&path).unwrap()==before;
  println!("cache={name} result={result:?} version={version} sentinel={sentinel:?} bytes_same={bytes_same}");
  preserved&=matches!(result,Err(ref e) if e.code=="KNOWLEDGE_SCHEMA_NEWER")&&version==999&&bytes_same;
 }
 assert!(preserved,"future原件拒绝与保留契约必须满足");
}
