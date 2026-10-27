-- 由真实旧生产adapter+旧forward-overlap kernel生成；不是手写预期数据库。

CREATE TABLE IF NOT EXISTS vector_index_variants (
    id                     TEXT PRIMARY KEY,
    chunk_index_variant_id TEXT NOT NULL,
    parse_artifact_id      TEXT NOT NULL,
    model_key              TEXT NOT NULL,
    chunk_fingerprint      TEXT NOT NULL,
    dimensions             INTEGER NOT NULL CHECK(dimensions > 0),
    status                 TEXT NOT NULL CHECK(status IN ('building','ready','failed')),
    UNIQUE (chunk_index_variant_id, model_key)
);
CREATE INDEX IF NOT EXISTS idx_vector_variants_artifact ON vector_index_variants(parse_artifact_id);
CREATE TABLE IF NOT EXISTS chunk_vectors (
    vector_index_variant_id TEXT NOT NULL REFERENCES vector_index_variants(id),
    chunk_id     TEXT NOT NULL,
    ordinal      INTEGER NOT NULL,
    vector       BLOB NOT NULL,
    PRIMARY KEY (vector_index_variant_id, chunk_id)
);

INSERT INTO vector_index_variants VALUES('viv_fe03b22ae55b57a84e850dde24e31d41','civ_58387c85d57aafdfa81b75b3fab17031','artifact','old-model','old-chunk-fingerprint',2,'ready');
INSERT INTO chunk_vectors VALUES('viv_fe03b22ae55b57a84e850dde24e31d41','chunk_a2fd0065d5205fa2f8df52c8e126d5e5',0,X'0000803F00000000');
INSERT INTO chunk_vectors VALUES('viv_fe03b22ae55b57a84e850dde24e31d41','chunk_808425e7b96122c67e103050a5b71a8b',1,X'0000803F00000000');
INSERT INTO chunk_vectors VALUES('viv_fe03b22ae55b57a84e850dde24e31d41','chunk_ecf4f13826e2cf735f5a657b578a1328',2,X'0000803F00000000');
PRAGMA user_version=1;
