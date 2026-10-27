-- 由真实旧生产adapter+旧forward-overlap kernel生成；不是手写预期数据库。

CREATE TABLE IF NOT EXISTS chunk_index_variants (
    id                 TEXT PRIMARY KEY,
    parse_artifact_id  TEXT NOT NULL,
    chunk_profile_hash TEXT NOT NULL,
    block_fingerprint  TEXT NOT NULL,
    status             TEXT NOT NULL CHECK(status IN ('building','ready','failed')),
    UNIQUE (parse_artifact_id, chunk_profile_hash)
);
CREATE TABLE IF NOT EXISTS knowledge_chunks (
    variant_id   TEXT NOT NULL REFERENCES chunk_index_variants(id),
    chunk_id     TEXT NOT NULL,
    ordinal      INTEGER NOT NULL,
    section_id   TEXT,
    text         TEXT NOT NULL,
    search_text  TEXT NOT NULL DEFAULT '',
    token_count  INTEGER NOT NULL,
    spans_json   TEXT NOT NULL DEFAULT '[]',
    PRIMARY KEY (variant_id, ordinal)
);
CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_chunks_fts USING fts5(
    text,
    search_text,
    content='knowledge_chunks',
    content_rowid='rowid',
    tokenize='unicode61'
);
CREATE TRIGGER IF NOT EXISTS chunks_fts_ai AFTER INSERT ON knowledge_chunks BEGIN
    INSERT INTO knowledge_chunks_fts(rowid, text, search_text) VALUES (new.rowid, new.text, new.search_text);
END;
CREATE TRIGGER IF NOT EXISTS chunks_fts_ad AFTER DELETE ON knowledge_chunks BEGIN
    INSERT INTO knowledge_chunks_fts(knowledge_chunks_fts, rowid, text, search_text) VALUES ('delete', old.rowid, old.text, old.search_text);
END;
CREATE TABLE IF NOT EXISTS knowledge_sections (
    id                  TEXT NOT NULL,
    parse_artifact_id   TEXT NOT NULL,
    section_ordinal     INTEGER NOT NULL,
    heading_path_json   TEXT NOT NULL DEFAULT '[]',
    start_block_ordinal INTEGER NOT NULL,
    end_block_ordinal   INTEGER NOT NULL,
    PRIMARY KEY (parse_artifact_id, section_ordinal)
);

INSERT INTO chunk_index_variants VALUES('civ_58387c85d57aafdfa81b75b3fab17031','artifact','25ba5b3326e6ed21','691e1ad1973fade418c2d23929cd456dcfbde5640e54946c519425376e70e3a0','ready');
INSERT INTO knowledge_chunks VALUES('civ_58387c85d57aafdfa81b75b3fab17031','chunk_a2fd0065d5205fa2f8df52c8e126d5e5',0,'section_80b67f7a5870dcf86b2eb45873103019','Ｑｕａｒｔｚ　aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','ｑｕａｒｔｚ aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa a',31,'[{"blockEndOffset":100,"blockId":"block","blockStartOffset":0,"chunkEndOffset":100,"chunkStartOffset":0}]');
INSERT INTO knowledge_chunks VALUES('civ_58387c85d57aafdfa81b75b3fab17031','chunk_808425e7b96122c67e103050a5b71a8b',1,'section_80b67f7a5870dcf86b2eb45873103019','bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb','bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb b',25,'[{"blockEndOffset":212,"blockId":"block","blockStartOffset":112,"chunkEndOffset":100,"chunkStartOffset":0}]');
INSERT INTO knowledge_chunks VALUES('civ_58387c85d57aafdfa81b75b3fab17031','chunk_ecf4f13826e2cf735f5a657b578a1328',2,'section_80b67f7a5870dcf86b2eb45873103019','bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb','bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb b',20,'[{"blockEndOffset":304,"blockId":"block","blockStartOffset":224,"chunkEndOffset":80,"chunkStartOffset":0}]');
INSERT INTO knowledge_chunks_fts(knowledge_chunks_fts) VALUES('rebuild');
PRAGMA user_version=1;
