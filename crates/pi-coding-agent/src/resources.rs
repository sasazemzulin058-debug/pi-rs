// ponytail: ceiling is memory-only resource metadata catalog; upgrade path is wiring to extensions and runtime.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceKind {
    Skill,
    Prompt,
    Theme,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceScope {
    User,
    Project,
    Temporary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceOrigin {
    Package,
    TopLevel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMetadata {
    pub source: String,
    pub scope: SourceScope,
    pub origin: SourceOrigin,
    pub base_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedResource {
    pub path: PathBuf,
    pub enabled: bool,
    pub metadata: PathMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceInfo {
    pub path: PathBuf,
    pub source: String,
    pub scope: SourceScope,
    pub origin: SourceOrigin,
    pub base_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceRecord {
    pub kind: ResourceKind,
    pub path: PathBuf,
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, Default)]
pub struct ExtensionResources {
    pub skills: Vec<ResolvedResource>,
    pub prompts: Vec<ResolvedResource>,
    pub themes: Vec<ResolvedResource>,
}

#[derive(Debug, Clone)]
pub struct ResourceCatalog {
    cwd: PathBuf,
    retained_base_metadata: HashMap<(ResourceKind, PathBuf), PathMetadata>,
    extension_metadata: HashMap<(ResourceKind, PathBuf), PathMetadata>,
    visible_paths: HashMap<ResourceKind, Vec<PathBuf>>,
}

impl ResourceCatalog {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            cwd,
            retained_base_metadata: HashMap::new(),
            extension_metadata: HashMap::new(),
            visible_paths: HashMap::new(),
        }
    }

    pub fn normalize_path(&self, p: &Path) -> PathBuf {
        if let Ok(c) = p.canonicalize() {
            return c;
        }
        let abs = if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.cwd.join(p)
        };
        let mut components = Vec::new();
        for comp in abs.components() {
            match comp {
                Component::CurDir => {}
                Component::ParentDir => {
                    components.pop();
                }
                c => components.push(c),
            }
        }
        components.iter().collect()
    }

    pub fn reload(
        &mut self,
        skills: Vec<ResolvedResource>,
        prompts: Vec<ResolvedResource>,
        themes: Vec<ResolvedResource>,
    ) {
        self.retained_base_metadata.clear();
        self.extension_metadata.clear();
        self.visible_paths.clear();

        let mut add_kind = |kind: ResourceKind, resources: Vec<ResolvedResource>| {
            let mut paths = Vec::new();
            for res in resources {
                let norm = self.normalize_path(&res.path);
                let mut meta = res.metadata;
                if let Some(ref bd) = meta.base_dir {
                    meta.base_dir = Some(self.normalize_path(bd));
                }
                self.retained_base_metadata
                    .insert((kind, norm.clone()), meta);
                if res.enabled && !paths.contains(&norm) {
                    paths.push(norm);
                }
            }
            self.visible_paths.insert(kind, paths);
        };

        add_kind(ResourceKind::Skill, skills);
        add_kind(ResourceKind::Prompt, prompts);
        add_kind(ResourceKind::Theme, themes);
    }

    pub fn extend_resources(&mut self, ext: ExtensionResources) {
        let mut add_ext = |kind: ResourceKind, resources: Vec<ResolvedResource>| {
            let mut new_paths = Vec::new();
            for res in resources {
                let norm = self.normalize_path(&res.path);
                let mut meta = res.metadata;
                if let Some(ref bd) = meta.base_dir {
                    meta.base_dir = Some(self.normalize_path(bd));
                }
                self.extension_metadata.insert((kind, norm.clone()), meta);
                if res.enabled {
                    new_paths.push(norm);
                }
            }
            let visible = self.visible_paths.entry(kind).or_default();
            for norm in new_paths {
                if !visible.contains(&norm) {
                    visible.push(norm);
                }
            }
        };

        add_ext(ResourceKind::Skill, ext.skills);
        add_ext(ResourceKind::Prompt, ext.prompts);
        add_ext(ResourceKind::Theme, ext.themes);
    }

    pub fn resolve_metadata(&self, kind: ResourceKind, path: &Path) -> Option<SourceInfo> {
        let norm_target = self.normalize_path(path);

        // Best match helper (longest matching path)
        let find_best =
            |map: &HashMap<(ResourceKind, PathBuf), PathMetadata>| -> Option<SourceInfo> {
                let mut best_match: Option<(&PathBuf, &PathMetadata)> = None;
                for ((k, meta_path), meta) in map {
                    if *k != kind {
                        continue;
                    }
                    if norm_target == *meta_path || norm_target.starts_with(meta_path) {
                        match best_match {
                            None => best_match = Some((meta_path, meta)),
                            Some((best_path, _)) => {
                                if meta_path.components().count() > best_path.components().count() {
                                    best_match = Some((meta_path, meta));
                                }
                            }
                        }
                    }
                }
                best_match.map(|(_, meta)| SourceInfo {
                    path: norm_target.clone(),
                    source: meta.source.clone(),
                    scope: meta.scope,
                    origin: meta.origin,
                    base_dir: meta.base_dir.clone(),
                })
            };

        if let Some(info) = find_best(&self.extension_metadata) {
            return Some(info);
        }
        find_best(&self.retained_base_metadata)
    }

    pub fn get_records(&self, kind: ResourceKind) -> Vec<ResourceRecord> {
        let mut records = Vec::new();
        if let Some(paths) = self.visible_paths.get(&kind) {
            for p in paths {
                if let Some(source_info) = self.resolve_metadata(kind, p) {
                    records.push(ResourceRecord {
                        kind,
                        path: p.clone(),
                        source_info,
                    });
                }
            }
        }
        records
    }
}
