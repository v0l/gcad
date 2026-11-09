use std::path::PathBuf;

fn skill() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("skills/gcad")
}

fn references() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(skill().join("references"))
        .expect("references folder")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "md"))
        .collect();
    files.sort();
    files
}

#[test]
fn every_reference_in_the_router_table_exists() {
    let router = std::fs::read_to_string(skill().join("SKILL.md")).expect("SKILL.md");
    let linked: Vec<&str> = router
        .split("](references/")
        .skip(1)
        .map(|rest| rest.split(')').next().expect("closing bracket"))
        .collect();
    assert!(linked.len() >= 10, "the router table lists {linked:?}");
    for name in &linked {
        assert!(
            skill().join("references").join(name).exists(),
            "SKILL.md links references/{name}, which does not exist"
        );
    }
    for file in references() {
        let name = file.file_name().unwrap().to_string_lossy().to_string();
        assert!(
            linked.contains(&name.as_str()),
            "references/{name} is not in the SKILL.md table"
        );
    }
}

#[test]
fn the_router_stays_short() {
    let router = std::fs::read_to_string(skill().join("SKILL.md")).expect("SKILL.md");
    assert!(router.lines().count() <= 120, "SKILL.md is a router");
}

#[test]
fn every_gcad_block_in_the_references_passes_check() {
    let scratch = std::env::temp_dir().join(format!("gcad-skill-doc-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("scratch folder");
    let mut checked = 0;
    let mut problems = Vec::new();
    for file in references() {
        let text = std::fs::read_to_string(&file).expect("reference");
        let mut block: Option<Vec<&str>> = None;
        for (index, line) in text.lines().enumerate() {
            match (&mut block, line.trim_end()) {
                (None, "```gcad") => block = Some(Vec::new()),
                (Some(lines), "```") => {
                    let path = scratch.join(format!("block{checked}.gcad"));
                    std::fs::write(&path, lines.join("\n") + "\n").expect("write block");
                    let out = std::process::Command::new(env!("CARGO_BIN_EXE_gcad"))
                        .arg("check")
                        .arg(&path)
                        .output()
                        .expect("run gcad");
                    if !out.status.success() {
                        problems.push(format!(
                            "{}:{}: {}{}",
                            file.display(),
                            index + 1,
                            String::from_utf8_lossy(&out.stdout),
                            String::from_utf8_lossy(&out.stderr)
                        ));
                    }
                    checked += 1;
                    block = None;
                }
                (Some(lines), _) => lines.push(line),
                _ => {}
            }
        }
        assert!(block.is_none(), "{} has an unclosed block", file.display());
    }
    let _ = std::fs::remove_dir_all(&scratch);
    assert!(checked >= 4, "only {checked} gcad blocks found");
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
