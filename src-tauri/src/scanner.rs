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

pub(crate) fn parse_frontmatter(contents: &str, fallback_name: &str) -> (String, String) {
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
mod tests;
