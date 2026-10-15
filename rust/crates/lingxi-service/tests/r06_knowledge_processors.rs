//! 真实候选子进程解析；位置和退出结果均读取实际输出。
use lingxi_service::knowledge_processors::parse_document;
use pdf_extract::{
    content::{Content, Operation},
    dictionary, Document, Object, Stream,
};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
fn root() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "lingxi-ex07-parser-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn bin() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_lingxi-service"))
}
fn pdf(texts: &[Option<&str>]) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages = doc.new_object_id();
    let font =
        doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"});
    let mut kids = Vec::new();
    for text in texts {
        let ops = match text {
            Some(text) => vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 12.into()]),
                Operation::new("Td", vec![72.into(), 720.into()]),
                Operation::new("Tj", vec![Object::string_literal(*text)]),
                Operation::new("ET", vec![]),
            ],
            None => vec![],
        };
        let stream = doc.add_object(Stream::new(
            dictionary! {},
            Content { operations: ops }.encode().unwrap(),
        ));
        let page=doc.add_object(dictionary!{"Type"=>"Page","Parent"=>pages,"MediaBox"=>vec![0.into(),0.into(),612.into(),792.into()],"Contents"=>stream,"Resources"=>dictionary!{"Font"=>dictionary!{"F1"=>font}}});
        kids.push(page.into());
    }
    doc.objects.insert(
        pages,
        dictionary! {"Type"=>"Pages","Kids"=>kids,"Count"=>texts.len() as i64}.into(),
    );
    let catalog = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
    doc.trailer.set("Root", catalog);
    doc.compress();
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}
#[test]
fn compressed_pdf_actual_pages_positions_and_ocr_status() {
    let parsed = parse_document(
        bin(),
        "application/pdf",
        &pdf(&[Some("First exact page"), Some("Second exact page")]),
    )
    .unwrap();
    assert_eq!(parsed.status, "ready");
    assert_eq!(parsed.fidelity, "citation_grade");
    assert_eq!(parsed.blocks.len(), 2);
    for (i, b) in parsed.blocks.iter().enumerate() {
        assert_eq!(
            b.text,
            if i == 0 {
                "First exact page"
            } else {
                "Second exact page"
            }
        );
        assert_eq!(b.locator["page"], i + 1);
        assert_eq!(b.locator["pageCharStart"], 0);
        assert_eq!(b.locator["pageCharEnd"], b.text.encode_utf16().count());
        let boxes = b.locator["boxes"].as_array().unwrap();
        assert!(!boxes.is_empty());
        assert_eq!(boxes[0]["x"], 72.0);
        assert_eq!(boxes[0]["y"], 720.0);
        assert!(boxes[0]["width"].as_f64().unwrap() > 0.0);
        assert_eq!(boxes[0]["boxKind"], "advance_cell");
    }
    let empty = parse_document(bin(), "application/pdf", &pdf(&[None])).unwrap();
    assert_eq!(empty.status, "needs_ocr");
    assert_eq!(empty.warnings, vec!["needs_ocr"]);
    assert!(empty.blocks.is_empty());
    assert!(parse_document(bin(), "application/pdf", b"%PDF-broken\xff").is_err());
}
#[test]
fn csv_html_real_structure_unicode_and_hidden_text() {
    let csv = parse_document(
        bin(),
        "text/csv",
        "a,\"quoted, value\"\n\"line\nvalue\",𠀀\n".as_bytes(),
    )
    .unwrap();
    assert_eq!(csv.fidelity, "structural");
    assert_eq!(csv.blocks[0].text, "a | quoted, value");
    assert_eq!(csv.blocks[1].text, "line value | 𠀀");
    assert_eq!(
        csv.blocks[1].locator["source"]["cells"],
        serde_json::json!(["R2C1", "R2C2"])
    );
    assert_eq!(
        csv.blocks[1].locator["charEnd"].as_u64().unwrap()
            - csv.blocks[1].locator["charStart"].as_u64().unwrap(),
        15
    );
    let html = parse_document(
        bin(),
        "text/html",
        b"<h1>Title</h1><div><p>visible<script>SECRET</script> text</p><p>second</p></div>",
    )
    .unwrap();
    assert_eq!(html.blocks.len(), 3);
    assert_eq!(html.blocks[1].text, "visible text");
    assert!(html.blocks[1].locator["structuralPath"]
        .as_str()
        .unwrap()
        .ends_with("p:nth-of-type(1)"));
    assert_eq!(
        html.blocks[2].locator["headingPath"],
        serde_json::json!(["Title"])
    );
}
#[cfg(unix)]
fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/usr/bin/python3\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
    p
}
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn physical_deadline_rss_output_limits_reap_child_and_next_parse_lives() {
    let dir = root();
    for(name,body,expected)in[("timeout",format!("import os,time\nopen({:?},'w').write(str(os.getpid()))\ntime.sleep(30)",dir.join("timeout.pid")),"five second"),("memory",format!("import os,time\nopen({:?},'w').write(str(os.getpid()))\na=bytearray(600*1024*1024)\ntime.sleep(30)",dir.join("memory.pid")),"memory"),("output",format!("import os,sys,time\nopen({:?},'w').write(str(os.getpid()))\nsys.stdout.buffer.write(b'x'*(17*1024*1024));sys.stdout.flush();time.sleep(30)",dir.join("output.pid")),"output")]{let executable=script(&dir,name,&body);let start=std::time::Instant::now();let error=parse_document(&executable,"application/pdf",b"x").unwrap_err();assert!(start.elapsed().as_secs()<8,"{name}: {error}");if name=="memory"{assert!(error.contains("RSS")||error.contains("unsuccessfully"),"{error}");}else{assert!(error.contains(expected),"{error}");}let pid:libc::pid_t=std::fs::read_to_string(dir.join(format!("{name}.pid"))).unwrap().parse().unwrap();assert_eq!(unsafe{libc::kill(pid,0)},-1,"child must be physically gone: {name}");}
    assert_eq!(
        parse_document(bin(), "application/pdf", &pdf(&[Some("still alive")]))
            .unwrap()
            .blocks[0]
            .text,
        "still alive"
    );
    std::fs::remove_dir_all(dir).unwrap();
}
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn actual_compressed_pdf_expansion_cannot_consume_unbounded_worker_memory() {
    let mut doc = Document::load_mem(&pdf(&[None])).unwrap();
    let page = *doc.get_pages().values().next().unwrap();
    let contents = doc
        .get_object(page)
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Contents")
        .unwrap()
        .as_reference()
        .unwrap();
    doc.objects.insert(
        contents,
        Stream::new(dictionary! {}, vec![b' '; 600 * 1024 * 1024]).into(),
    );
    doc.compress();
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    drop(doc);
    assert!(bytes.len() < 1024 * 1024, "真实压缩扩张输入应小于1MiB");
    let started = std::time::Instant::now();
    let error = parse_document(bin(), "application/pdf", &bytes).unwrap_err();
    eprintln!(
        "EX07_PDF_BUDGET input_bytes={} expanded_bytes={} elapsed_ms={} refusal={error}",
        bytes.len(),
        600 * 1024 * 1024,
        started.elapsed().as_millis()
    );
    assert!(started.elapsed().as_secs() < 8, "{error}");
    assert!(
        error.contains("RSS")
            || error.contains("five second")
            || error.contains("unsuccessfully")
            || error.contains("PDF text decode"),
        "{error}"
    );
    assert_eq!(
        parse_document(bin(), "application/pdf", &pdf(&[Some("after bomb")]))
            .unwrap()
            .blocks[0]
            .text,
        "after bomb"
    );
}
#[test]
fn unicode_cmap_and_reused_page_font_name_keep_real_text_and_utf16_locations() {
    let mut doc = Document::with_version("1.5");
    let pages = doc.new_object_id();
    let mut kids = Vec::new();
    for map in ["4E2D", "6587"] {
        let cmap=format!("/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def /CMapName /R06 def /CMapType 2 def 1 begincodespacerange <0000> <FFFF> endcodespacerange 2 beginbfchar <0001> <{map}> <0002> <D840DC00> endbfchar endcmap CMapName currentdict /CMap defineresource pop end end");
        let unicode = doc.add_object(Stream::new(dictionary! {}, cmap.into_bytes()));
        let descriptor=doc.add_object(dictionary!{"Type"=>"FontDescriptor","FontName"=>"R06Synthetic","Flags"=>4,"FontBBox"=>vec![0.into(),0.into(),1000.into(),1000.into()],"ItalicAngle"=>0,"Ascent"=>880,"Descent"=>-120,"CapHeight"=>700,"StemV"=>80});
        let descendant=doc.add_object(dictionary!{"Type"=>"Font","Subtype"=>"CIDFontType2","BaseFont"=>"R06Synthetic","CIDSystemInfo"=>dictionary!{"Registry"=>Object::string_literal("Adobe"),"Ordering"=>Object::string_literal("Identity"),"Supplement"=>0},"FontDescriptor"=>descriptor,"DW"=>1000});
        let font=doc.add_object(dictionary!{"Type"=>"Font","Subtype"=>"Type0","BaseFont"=>"R06Synthetic","Encoding"=>"Identity-H","DescendantFonts"=>vec![descendant.into()],"ToUnicode"=>unicode});
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 12.into()]),
                Operation::new("Td", vec![72.into(), 720.into()]),
                Operation::new("Tj", vec![Object::string_literal(vec![0, 1, 0, 2])]),
                Operation::new("ET", vec![]),
            ],
        }
        .encode()
        .unwrap();
        let stream = doc.add_object(Stream::new(dictionary! {}, content));
        let page=doc.add_object(dictionary!{"Type"=>"Page","Parent"=>pages,"MediaBox"=>vec![0.into(),0.into(),612.into(),792.into()],"Contents"=>stream,"Resources"=>dictionary!{"Font"=>dictionary!{"F1"=>font}}});
        kids.push(page.into());
    }
    doc.objects.insert(
        pages,
        dictionary! {"Type"=>"Pages","Kids"=>kids,"Count"=>2}.into(),
    );
    let catalog = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
    doc.trailer.set("Root", catalog);
    doc.compress();
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let parsed = parse_document(bin(), "application/pdf", &bytes).unwrap();
    assert_eq!(
        parsed
            .blocks
            .iter()
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>(),
        vec!["中𠀀", "文𠀀"]
    );
    for block in parsed.blocks {
        assert_eq!(block.locator["pageCharEnd"], 3);
        assert_eq!(block.locator["boxes"].as_array().unwrap().len(), 2);
    }
}
#[test]
fn production_typescript_processor_reference_matches_structured_blocks() {
    let expected: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/r06_knowledge/ts-reference.json")).unwrap();
    for (ext, mime, bytes) in [
        (
            "docx",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            include_bytes!("fixtures/r06_knowledge/reference.docx").as_slice(),
        ),
        (
            "xlsx",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            include_bytes!("fixtures/r06_knowledge/reference.xlsx").as_slice(),
        ),
        (
            "csv",
            "text/csv",
            include_bytes!("fixtures/r06_knowledge/reference.csv").as_slice(),
        ),
    ] {
        let actual = parse_document(bin(), mime, bytes).unwrap();
        let blocks = expected[ext]["blocks"].as_array().unwrap();
        assert_eq!(actual.blocks.len(), blocks.len(), "{ext}");
        for (a, e) in actual.blocks.iter().zip(blocks) {
            assert_eq!(a.text, e["text"], "{ext}");
            assert_eq!(a.locator_type, e["locatorType"], "{ext}");
            assert_eq!(a.locator, e["locator"], "{ext}");
        }
        assert_eq!(actual.fidelity, expected[ext]["fidelity"]);
    }
}

#[test]
fn current_typescript_csv_coercion_empty_rows_and_invalid_quote_reference() {
    let reference: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/r06_knowledge/ts-reference.json")).unwrap();
    for case in reference["csvCases"].as_array().unwrap() {
        let actual = parse_document(
            bin(),
            "text/csv",
            case["input"].as_str().unwrap().as_bytes(),
        );
        if case.get("error").is_some() {
            assert!(actual.is_err(), "{case}");
        } else {
            let actual = actual.unwrap();
            let expected = case["blocks"].as_array().unwrap();
            assert_eq!(actual.blocks.len(), expected.len(), "{case}");
            for (a, e) in actual.blocks.iter().zip(expected) {
                assert_eq!(a.text, e["text"], "{case}");
                assert_eq!(a.locator, e["locator"], "{case}");
            }
        }
    }
}
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn trusted_cancellation_kills_and_reaps_the_actual_parser_before_return() {
    let directory = root();
    let pidfile = directory.join("cancel.pid");
    let helper = script(
        &directory,
        "cancel-worker",
        &format!("import os,time\nopen({pidfile:?},'w').write(str(os.getpid()))\ntime.sleep(30)"),
    );
    let active = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let changed = active.clone();
    let path = pidfile.clone();
    let trigger = std::thread::spawn(move || {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !path.exists() {
            assert!(std::time::Instant::now() < until);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        changed.store(false, Ordering::Release);
    });
    let start = std::time::Instant::now();
    let error = lingxi_service::knowledge_processors::parse_document_while_active(
        &helper,
        "application/pdf",
        b"",
        || active.load(Ordering::Acquire),
    )
    .unwrap_err();
    trigger.join().unwrap();
    assert_eq!(error, "document parser cancelled");
    assert!(start.elapsed().as_secs() < 2);
    let pid = std::fs::read_to_string(pidfile)
        .unwrap()
        .parse::<libc::pid_t>()
        .unwrap();
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    assert_eq!(
        parse_document(bin(), "application/pdf", &pdf(&[Some("after cancel")]))
            .unwrap()
            .blocks[0]
            .text,
        "after cancel"
    );
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn sparse_workbook_processing_counts_actual_cells_without_rendering_a_full_grid() {
    use std::io::Write;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let rows=(1..=20).map(|n|format!("<row r=\"{n}\"><c r=\"XFD{n}\" t=\"inlineStr\"><is><t>SPARSE_{n}</t></is></c></row>")).collect::<String>();
    for(name,text)in[("xl/workbook.xml","<workbook xmlns:r=\"r\"><sheets><sheet name=\"Sparse\" r:id=\"s\"/></sheets></workbook>".into()),("xl/_rels/workbook.xml.rels","<Relationships><Relationship Id=\"s\" Type=\"x/worksheet\" Target=\"worksheets/a.xml\"/></Relationships>".into()),("xl/worksheets/a.xml",format!("<worksheet><sheetData>{rows}</sheetData></worksheet>"))]{writer.start_file(name,zip::write::SimpleFileOptions::default()).unwrap();writer.write_all(text.as_bytes()).unwrap();}
    let bytes = writer.finish().unwrap().into_inner();
    let parsed = parse_document(
        bin(),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        &bytes,
    )
    .unwrap();
    assert_eq!(parsed.blocks.len(), 20);
    assert!(parsed.warnings.is_empty());
    assert_eq!(
        parsed.blocks[19].locator["source"]["cells"],
        serde_json::json!(["XFD20"])
    );
}
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn concurrent_parser_refuses_without_spawning_until_physical_cancellation_releases_lease() {
    let directory = root();
    let pidfile = directory.join("lease.pid");
    let helper = script(
        &directory,
        "lease-worker",
        &format!("import os,time\nopen({pidfile:?},'w').write(str(os.getpid()))\ntime.sleep(30)"),
    );
    let active = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let run_active = active.clone();
    let first = std::thread::spawn(move || {
        lingxi_service::knowledge_processors::parse_document_while_active(
            &helper,
            "application/pdf",
            b"",
            || run_active.load(Ordering::Acquire),
        )
    });
    let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !pidfile.exists() {
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let start = std::time::Instant::now();
    assert!(
        parse_document(bin(), "application/pdf", &pdf(&[Some("must not spawn")]))
            .unwrap_err()
            .contains("busy")
    );
    assert!(start.elapsed().as_millis() < 500);
    active.store(false, Ordering::Release);
    assert_eq!(
        first.join().unwrap().unwrap_err(),
        "document parser cancelled"
    );
    let pid = std::fs::read_to_string(pidfile)
        .unwrap()
        .parse::<libc::pid_t>()
        .unwrap();
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    assert_eq!(
        parse_document(bin(), "application/pdf", &pdf(&[Some("lease released")]))
            .unwrap()
            .blocks[0]
            .text,
        "lease released"
    );
    std::fs::remove_dir_all(directory).unwrap();
}
