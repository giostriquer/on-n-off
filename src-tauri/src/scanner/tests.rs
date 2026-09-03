use super::*;
use std::fs;

#[test]
fn scans_nested_skills_and_ignores_junk_folders() {
    let root = std::env::temp_dir().join(format!("on-n-off-scan-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("skills/alpha")).unwrap();
    fs::write(
        root.join("skills/alpha/SKILL.md"),
        "---\nname: alpha\ndescription: Alpha skill\n---\nbody\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("skills/junk")).unwrap();
    fs::create_dir_all(root.join("skills/beta")).unwrap();
    fs::write(root.join("skills/beta/SKILL.md"), "# beta only\n").unwrap();

    let skills = scan_plugin_skills(&root);
    let names: Vec<_> = skills.iter().map(|skill| skill.name.as_str()).collect();
    assert_eq!(names, ["alpha", "beta"]);
    assert_eq!(skills[0].description, "Alpha skill");
    assert!(skills[1].description.is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn scans_root_skill_md_when_skills_dir_missing() {
    let root = std::env::temp_dir().join(format!("on-n-off-root-skill-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("SKILL.md"),
        "---\nname: solo\ndescription: One shot\n---\n",
    )
    .unwrap();
    let skills = scan_plugin_skills(&root);
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].name, "solo");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn scans_flat_markdown_skills_for_antigravity() {
    let root = std::env::temp_dir().join(format!("on-n-off-flat-skill-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("solo.md"),
        "---\nname: solo\ndescription: Flat\n---\n",
    )
    .unwrap();
    fs::write(root.join("SKILL.md"), "---\nname: ignore-root\n---\n").unwrap();
    assert!(scan_user_skills(&root).is_empty());
    let skills = scan_antigravity_skills(&root);
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].name, "solo");
    let _ = fs::remove_dir_all(root);
}
