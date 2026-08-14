use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedSkill {
    pub name: String,
    pub description: String,
    pub skill_md: PathBuf,
}

pub fn scan_plugin_skills(plugin_root: &Path) -> Vec<ScannedSkill> {
    let skills_dir = plugin_root.join("skills");
    let mut skills = if skills_dir.is_dir() {
        scan_skills_dir(&skills_dir)
    } else {
        Vec::new()
    };
    let root_skill = plugin_root.join("SKILL.md");
    if skills.is_empty() && root_skill.is_file() {
        if let Some(skill) = skill_from_file(&root_skill, plugin_root) {
            skills.push(skill);
        }
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

pub fn scan_user_skills(skills_root: &Path) -> Vec<ScannedSkill> {
    if !skills_root.is_dir() {
        return Vec::new();
    }
    let mut skills = scan_skills_dir(skills_root);
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// Flat `name.md` skills (Antigravity CLI / workspace style).
pub fn scan_skill_markdown_files(dir: &Path) -> Vec<ScannedSkill> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut skills = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        if lower == "skill.md" || !lower.ends_with(".md") {
            continue;
        }
        if let Some(skill) = skill_from_file(&path, &path.with_extension("")) {
            skills.push(skill);
        }
    }
    skills
}

pub fn scan_antigravity_skills(skills_root: &Path) -> Vec<ScannedSkill> {
    if !skills_root.is_dir() {
        return Vec::new();
    }
    let mut skills = scan_skills_dir(skills_root);
    skills.extend(scan_skill_markdown_files(skills_root));
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

fn scan_skills_dir(dir: &Path) -> Vec<ScannedSkill> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut skills = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        if skill_md.is_file() {
            if let Some(skill) = skill_from_file(&skill_md, &path) {
                skills.push(skill);
            }
        }
    }
    skills
}

pub fn scan_skill_md(skill_md: &Path) -> Option<ScannedSkill> {
    let folder = skill_md.parent().unwrap_or(skill_md);
    skill_from_file(skill_md, folder)
}

fn skill_from_file(skill_md: &Path, folder: &Path) -> Option<ScannedSkill> {
    let contents = fs::read_to_string(skill_md).ok()?;
    let folder_name = folder
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("skill");
    let (name, description) = parse_frontmatter(&contents, folder_name);
    Some(ScannedSkill {
        name,
        description,
        skill_md: skill_md.to_path_buf(),
    })
}

fn parse_frontmatter(contents: &str, fallback_name: &str) -> (String, String) {
    let Some(rest) = contents.strip_prefix("---") else {
        return (fallback_name.to_string(), String::new());
    };
    let Some((fm, _)) = rest.split_once("\n---") else {
        return (fallback_name.to_string(), String::new());
    };
    let mut name = fallback_name.to_string();
    let mut description = String::new();
    for line in fm.lines() {
        if let Some(value) = line.strip_prefix("name:") {
            let value = value.trim().trim_matches('"').trim_matches('\'');
            if !value.is_empty() {
                name = value.to_string();
            }
        }
        if let Some(value) = line.strip_prefix("description:") {
            description = value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
        }
    }
    (name, description)
}

#[cfg(test)]
mod tests {
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
}
