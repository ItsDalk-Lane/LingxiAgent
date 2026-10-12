use lingxi_adapters::storage::knowledge::{KnowledgeIndexStore, KnowledgeVectorStore};
fn main() {
    eprintln!("guard={:?}",lingxi_test_guard::state());
    let home=std::path::PathBuf::from(std::env::var_os("LINGXI_HOME").unwrap());
    for name in ["knowledge_index.db","knowledge_vector.db"] {
        let root=home.join(name);std::fs::create_dir_all(&root).unwrap();
        if name=="knowledge_index.db" {drop(KnowledgeIndexStore::open(&root).unwrap());} else {drop(KnowledgeVectorStore::open(&root).unwrap());}
        let file=root.join(name);let files=std::fs::read_dir(&root).unwrap().map(|x|x.unwrap().file_name().to_string_lossy().to_string()).collect::<Vec<_>>();
        eprintln!("after supported normal close name={name} files={files:?}");
        std::fs::write(&file,b"damaged-owned-synthetic-cache").unwrap();
        let recovered=if name=="knowledge_index.db" {KnowledgeIndexStore::open(&root).map(|_| ())} else {KnowledgeVectorStore::open(&root).map(|_| ())};
        eprintln!("actual recovery={recovered:?}");assert!(recovered.is_ok(),"正常关闭后既有自有静止损坏恢复必须有效");
    }
}
