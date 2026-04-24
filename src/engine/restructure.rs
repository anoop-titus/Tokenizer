use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config::claude_dir;
use crate::engine::scanner::ScannedFile;

/// A node in a directory tree representation.
#[derive(Debug, Clone)]
pub struct TreeNode {
    pub name: String,
    pub is_dir: bool,
    pub kind: TreeNodeKind,
    pub children: Vec<TreeNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeNodeKind {
    Existing,
    New,
    Moved,
    #[allow(dead_code)]
    Removed,
}

/// Build a tree representation of the current ~/.claude directory structure (2-3 levels deep).
pub fn build_current_tree() -> Vec<TreeNode> {
    let cd = claude_dir();
    if !cd.exists() {
        return vec![TreeNode {
            name: ".claude/".to_string(),
            is_dir: true,
            kind: TreeNodeKind::Existing,
            children: vec![],
        }];
    }

    let mut root_children = Vec::new();
    let subdirs = ["rules", "agents", "skills", "commands", "projects", "hooks"];

    for subdir in &subdirs {
        let path = cd.join(subdir);
        if !path.exists() {
            continue;
        }
        let mut dir_node = TreeNode {
            name: subdir.to_string(),
            is_dir: true,
            kind: TreeNodeKind::Existing,
            children: Vec::new(),
        };

        // Read 1 level of children
        if let Ok(entries) = std::fs::read_dir(&path) {
            let mut children: Vec<TreeNode> = entries
                .flatten()
                .map(|e| {
                    let is_dir = e.path().is_dir();
                    let name = e.file_name().to_string_lossy().to_string();
                    let mut node = TreeNode {
                        name,
                        is_dir,
                        kind: TreeNodeKind::Existing,
                        children: Vec::new(),
                    };
                    // One more level for dirs
                    if is_dir {
                        if let Ok(sub_entries) = std::fs::read_dir(e.path()) {
                            node.children = sub_entries
                                .flatten()
                                .map(|se| TreeNode {
                                    name: se.file_name().to_string_lossy().to_string(),
                                    is_dir: se.path().is_dir(),
                                    kind: TreeNodeKind::Existing,
                                    children: Vec::new(),
                                })
                                .collect();
                            node.children.sort_by(|a, b| a.name.cmp(&b.name));
                        }
                    }
                    node
                })
                .collect();
            children.sort_by(|a, b| a.name.cmp(&b.name));
            dir_node.children = children;
        }

        root_children.push(dir_node);
    }

    // Top-level files
    if let Ok(entries) = std::fs::read_dir(&cd) {
        for entry in entries.flatten() {
            if entry.path().is_file() {
                root_children.push(TreeNode {
                    name: entry.file_name().to_string_lossy().to_string(),
                    is_dir: false,
                    kind: TreeNodeKind::Existing,
                    children: Vec::new(),
                });
            }
        }
    }

    root_children.sort_by(|a, b| {
        // dirs first, then files
        match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        }
    });

    vec![TreeNode {
        name: ".claude".to_string(),
        is_dir: true,
        kind: TreeNodeKind::Existing,
        children: root_children,
    }]
}

/// Build a proposed reorganized tree based on scan results.
/// Groups agents by prefix/domain.
pub fn build_proposed_tree(scan_results: &[ScannedFile]) -> Vec<TreeNode> {
    let mut root_children = Vec::new();

    // Group agents by prefix
    let agent_groups = group_agents_by_prefix(scan_results);
    if !agent_groups.is_empty() {
        let mut agents_node = TreeNode {
            name: "agents".to_string(),
            is_dir: true,
            kind: TreeNodeKind::Existing,
            children: Vec::new(),
        };

        for (group_name, files) in &agent_groups {
            if files.len() > 1 {
                // Create subdirectory for the group
                let mut group_node = TreeNode {
                    name: group_name.clone(),
                    is_dir: true,
                    kind: TreeNodeKind::New,
                    children: Vec::new(),
                };
                for file_name in files {
                    group_node.children.push(TreeNode {
                        name: file_name.clone(),
                        is_dir: false,
                        kind: TreeNodeKind::Moved,
                        children: Vec::new(),
                    });
                }
                agents_node.children.push(group_node);
            } else {
                // Single file stays at top level
                for file_name in files {
                    agents_node.children.push(TreeNode {
                        name: file_name.clone(),
                        is_dir: false,
                        kind: TreeNodeKind::Existing,
                        children: Vec::new(),
                    });
                }
            }
        }
        root_children.push(agents_node);
    }

    // Keep other directories as-is but add them
    for dir_name in ["rules", "skills", "commands", "projects", "hooks"] {
        let path = claude_dir().join(dir_name);
        if path.exists() {
            root_children.push(TreeNode {
                name: dir_name.to_string(),
                is_dir: true,
                kind: TreeNodeKind::Existing,
                children: Vec::new(),
            });
        }
    }

    // Propose moving loose doc files (.md, .toon at top level) into docs/
    let cd = claude_dir();
    let keep_at_root = ["CLAUDE.md", "MEMORY.md"];
    if let Ok(entries) = std::fs::read_dir(&cd) {
        let loose_docs: Vec<String> = entries
            .flatten()
            .filter(|e| {
                if !e.path().is_file() {
                    return false;
                }
                let name = e.file_name().to_string_lossy().to_string();
                if keep_at_root.contains(&name.as_str()) {
                    return false;
                }
                let ext = e
                    .path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .unwrap_or("")
                    .to_string();
                ext == "md" || ext == "toon"
            })
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();

        if !loose_docs.is_empty() {
            let mut docs_node = TreeNode {
                name: "docs".to_string(),
                is_dir: true,
                kind: TreeNodeKind::New,
                children: Vec::new(),
            };
            for name in loose_docs {
                docs_node.children.push(TreeNode {
                    name,
                    is_dir: false,
                    kind: TreeNodeKind::Moved,
                    children: Vec::new(),
                });
            }
            root_children.push(docs_node);
        }
    }

    root_children.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });

    vec![TreeNode {
        name: ".claude".to_string(),
        is_dir: true,
        kind: TreeNodeKind::Existing,
        children: root_children,
    }]
}

/// Apply the proposed restructure tree to disk.
/// Creates new directories and moves files from their current locations.
pub fn apply_restructure(proposed: &[TreeNode], base_dir: &Path) -> anyhow::Result<Vec<String>> {
    let mut actions = Vec::new();
    // The proposed tree root node is ".claude" which IS base_dir.
    // Skip the root node and process its children directly to avoid double-nesting.
    for root_node in proposed {
        if root_node.is_dir && !root_node.children.is_empty() {
            apply_tree_recursive(&root_node.children, base_dir, base_dir, &mut actions)?;
        }
    }
    Ok(actions)
}

fn apply_tree_recursive(
    nodes: &[TreeNode],
    parent: &Path,
    base_dir: &Path,
    actions: &mut Vec<String>,
) -> anyhow::Result<()> {
    for node in nodes {
        let dest_path = parent.join(&node.name);
        match node.kind {
            TreeNodeKind::New if node.is_dir => {
                if !dest_path.exists() {
                    std::fs::create_dir_all(&dest_path)?;
                    actions.push(format!("mkdir {}", dest_path.display()));
                }
            }
            TreeNodeKind::Moved if !node.is_dir => {
                // Already at destination — skip (idempotency)
                if dest_path.exists() {
                    // Already restructured, nothing to do
                } else if let Some(source) = find_source_file(base_dir, &node.name) {
                    if source != dest_path && source.exists() {
                        // Ensure parent dir exists
                        if let Some(p) = dest_path.parent() {
                            std::fs::create_dir_all(p)?;
                        }
                        std::fs::rename(&source, &dest_path)?;
                        actions.push(format!(
                            "move {} -> {}",
                            source.display(),
                            dest_path.display()
                        ));

                        // Clean up empty source directory
                        if let Some(src_parent) = source.parent() {
                            if src_parent != base_dir {
                                if let Ok(entries) = std::fs::read_dir(src_parent) {
                                    if entries.count() == 0 {
                                        let _ = std::fs::remove_dir(src_parent);
                                        actions.push(format!(
                                            "rmdir (empty) {}",
                                            src_parent.display()
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        if !node.children.is_empty() {
            apply_tree_recursive(&node.children, &dest_path, base_dir, actions)?;
        }
    }
    Ok(())
}

/// Search for a file by name in known subdirectories of base_dir.
fn find_source_file(base_dir: &Path, filename: &str) -> Option<PathBuf> {
    // Check top-level first
    let top = base_dir.join(filename);
    if top.exists() {
        return Some(top);
    }
    // Search known subdirs
    for subdir in ["agents", "commands", "rules", "skills", "projects", "hooks"] {
        let path = base_dir.join(subdir).join(filename);
        if path.exists() {
            return Some(path);
        }
        // One level deeper
        if let Ok(entries) = std::fs::read_dir(base_dir.join(subdir)) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    let deep = entry.path().join(filename);
                    if deep.exists() {
                        return Some(deep);
                    }
                }
            }
        }
    }
    None
}

/// Group agent files by their prefix (e.g., gsd-*, engineering-*, etc.).
fn group_agents_by_prefix(scan_results: &[ScannedFile]) -> BTreeMap<String, Vec<String>> {
    use crate::engine::scanner::FileCategory;

    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for file in scan_results {
        if file.category != FileCategory::Agent {
            continue;
        }
        let name = file
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        // Extract prefix: everything before the first '-'
        let prefix = if let Some(pos) = name.find('-') {
            let p = &name[..pos];
            // Map common prefixes to domain names
            match p {
                "gsd" => "workflow".to_string(),
                "engineering" | "eng" => "engineering".to_string(),
                "marketing" | "mkt" => "marketing".to_string(),
                "testing" | "test" | "qa" => "testing".to_string(),
                "healthcare" | "health" => "healthcare".to_string(),
                "finance" | "fin" | "trading" => "finance".to_string(),
                other => other.to_string(),
            }
        } else {
            "general".to_string()
        };

        groups.entry(prefix).or_default().push(name);
    }

    groups
}
