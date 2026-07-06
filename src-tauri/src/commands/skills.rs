use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;
use tauri::State;
use walkdir::WalkDir;

use crate::core::{
    audit_log::{AuditDraft, AuditEntry},
    central_repo,
    content_hash,
    error::AppError,
    git_fetcher,
    install_cancel::InstallCancelRegistry,
    installer,
    repo_lock::RepoLock,
    scanner,
    scenario_service,
    skill_metadata::{self, is_valid_skill_dir},
    skill_store::{SkillRecord, SkillStore, SkillTargetRecord},
    sync_engine, sync_metadata,
    timing::should_log_first_or_slow,
};

const MAX_EDITABLE_FILE_BYTES: u64 = 1_000_000;
const MAX_MARKET_PREVIEW_BYTES: u64 = 2_000_000;
const MAX_FILE_TREE_DEPTH: usize = 8;

#[derive(Debug, Serialize)]
pub struct UpdateSkillResult {
    pub skill: ManagedSkillDto,
    /// Whether the skill's file content actually changed.
    /// False when a monorepo commit didn't touch this skill's subdirectory.
    pub content_changed: bool,
}

#[derive(Debug, Serialize)]
pub struct BatchUpdateSkillsResult {
    pub refreshed: usize,
    pub unchanged: usize,
    pub failed: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct BatchDeleteSkillsResult {
    pub deleted: usize,
    pub failed: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ManagedSkillDto {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub source_type: String,
    pub source_ref: Option<String>,
    pub source_ref_resolved: Option<String>,
    pub source_subpath: Option<String>,
    pub source_branch: Option<String>,
    pub source_revision: Option<String>,
    pub remote_revision: Option<String>,
    pub update_status: String,
    pub last_checked_at: Option<i64>,
    pub last_check_error: Option<String>,
    pub central_path: String,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub status: String,
    pub targets: Vec<TargetDto>,
    pub preset_ids: Vec<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct TargetDto {
    pub id: String,
    pub skill_id: String,
    pub tool: String,
    pub target_path: String,
    pub mode: String,
    pub status: String,
    pub synced_at: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct SkillDocumentDto {
    pub skill_id: String,
    pub filename: String,
    pub content: String,
    pub central_path: String,
}

#[derive(Debug, Serialize)]
pub struct SourceSkillDocumentDto {
    pub skill_id: String,
    pub filename: String,
    pub content: String,
    pub source_label: String,
    pub revision: String,
}

/// Whole-directory diff between the central copy (`original`) and the source
/// (`updated`), covering the same file scope that drives the update badge so
/// the diff can never come back empty while the badge says "update available".
#[derive(Debug, Serialize)]
pub struct SkillSourceDiffDto {
    pub skill_id: String,
    pub source_label: String,
    pub revision: String,
    pub entries: Vec<SkillSourceDiffEntryDto>,
}

#[derive(Debug, Serialize)]
pub struct SkillSourceDiffEntryDto {
    pub relative_path: String,
    /// "added" | "removed" | "modified"
    pub status: String,
    /// "text" | "binary" | "too_large" | "permission_only"
    pub content_kind: String,
    /// Present only when `content_kind == "text"`.
    pub original_text: Option<String>,
    pub updated_text: Option<String>,
    pub executable_before: bool,
    pub executable_after: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct SkillFileNodeDto {
    pub name: String,
    pub relative_path: String,
    /// "file" | "directory"
    pub kind: String,
    pub size: Option<u64>,
    pub modified_at: Option<i64>,
    pub children: Option<Vec<SkillFileNodeDto>>,
}

#[derive(Debug, Serialize)]
pub struct SkillFileContentDto {
    pub skill_id: String,
    pub relative_path: String,
    pub content: String,
    pub size: u64,
    pub modified_at: Option<i64>,
    pub hash: String,
}

#[derive(Debug, Serialize)]
pub struct SkillFileDiffDto {
    pub skill_id: String,
    pub relative_path: String,
    pub original_text: String,
    pub updated_text: String,
    pub original_hash: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct SkillQualityIssueDto {
    /// "error" | "warning" | "info"
    pub severity: String,
    pub code: String,
    pub message: String,
    pub relative_path: Option<String>,
    pub line: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct MarketSkillPreviewDto {
    pub source: String,
    pub skill_id: String,
    pub name: String,
    pub description: Option<String>,
    pub files: Vec<SkillFileNodeDto>,
    pub document: Option<SkillFileContentDto>,
    pub file_previews: Vec<MarketSkillFilePreviewDto>,
    pub risk_issues: Vec<SkillQualityIssueDto>,
}

#[derive(Debug, Serialize)]
pub struct MarketSkillFilePreviewDto {
    pub relative_path: String,
    pub content: String,
    pub size: u64,
    pub hash: String,
}

#[derive(Debug, Serialize)]
pub struct SkillAuditEntryDto {
    pub id: i64,
    pub ts: i64,
    pub action: String,
    pub tool: Option<String>,
    pub success: bool,
    pub detail: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SkillEditSnapshotDto {
    pub id: String,
    pub ts: i64,
    pub skill_id: String,
    pub skill_name: String,
    pub relative_path: String,
    pub original_hash: String,
    pub size: u64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct SkillEditSnapshotMeta {
    id: String,
    ts: i64,
    skill_id: String,
    skill_name: String,
    relative_path: String,
    original_hash: String,
    size: u64,
}

#[derive(Debug, Clone)]
pub struct InstallSourceMetadata {
    pub source_type: String,
    pub source_ref: Option<String>,
    pub source_ref_resolved: Option<String>,
    pub source_subpath: Option<String>,
    pub source_branch: Option<String>,
    pub source_revision: Option<String>,
    pub remote_revision: Option<String>,
    pub update_status: String,
}

#[derive(Debug, Clone)]
pub struct GitSkillSource {
    pub clone_url: String,
    pub branch: Option<String>,
    pub subpath: Option<String>,
    pub locator_skill_id: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct GitSkillPreview {
    /// Path relative to the resolved scan root, using `/` separators. Stable key.
    pub rel_path: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct GitPreviewResult {
    pub temp_dir: String,
    pub skills: Vec<GitSkillPreview>,
}

#[derive(Debug, serde::Deserialize)]
pub struct SkillInstallItem {
    pub rel_path: String,
    pub name: String,
}

struct CancelRegistrationGuard {
    registry: Arc<InstallCancelRegistry>,
    key: String,
}

impl CancelRegistrationGuard {
    fn new(registry: Arc<InstallCancelRegistry>, key: String) -> Self {
        Self { registry, key }
    }
}

impl Drop for CancelRegistrationGuard {
    fn drop(&mut self) {
        self.registry.remove(&self.key);
    }
}

static GET_MANAGED_SKILLS_FIRST_CALL: AtomicBool = AtomicBool::new(true);

#[tauri::command]
pub async fn get_managed_skills(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<ManagedSkillDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let start = Instant::now();
        let skills = store.get_all_skills().map_err(AppError::db)?;
        let all_targets = store.get_all_targets().map_err(AppError::db)?;
        let tags_map = store.get_tags_map().map_err(AppError::db)?;
        let count = skills.len();
        let dtos: Vec<ManagedSkillDto> = skills
            .into_iter()
            .map(|skill| managed_skill_to_dto(&store, skill, &all_targets, &tags_map))
            .collect();
        let elapsed_ms = start.elapsed().as_millis();
        if should_log_first_or_slow(&GET_MANAGED_SKILLS_FIRST_CALL, elapsed_ms, 100) {
            log::info!("get_managed_skills: {count} skills in {elapsed_ms} ms");
        }
        Ok(dtos)
    })
    .await?
}

#[tauri::command]
pub async fn get_skills_for_preset(
    preset_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<ManagedSkillDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skills = store
            .get_skills_for_scenario(&preset_id)
            .map_err(AppError::db)?;
        let all_targets = store.get_all_targets().map_err(AppError::db)?;
        let tags_map = store.get_tags_map().map_err(AppError::db)?;

        Ok(skills
            .into_iter()
            .map(|skill| managed_skill_to_dto(&store, skill, &all_targets, &tags_map))
            .collect())
    })
    .await?
}

#[tauri::command]
pub async fn get_skill_document(
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<SkillDocumentDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = store
            .get_skill_by_id(&skill_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Skill not found"))?;

        let (filename, content) = read_skill_document_from_dir(Path::new(&skill.central_path))?;

        Ok(SkillDocumentDto {
            skill_id,
            filename,
            content,
            central_path: skill.central_path,
        })
    })
    .await?
}

#[tauri::command]
pub async fn get_skill_file_tree(
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<SkillFileNodeDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = load_skill(&store, &skill_id)?;
        build_skill_file_tree(Path::new(&skill.central_path))
    })
    .await?
}

#[tauri::command]
pub async fn read_skill_file(
    skill_id: String,
    relative_path: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<SkillFileContentDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = load_skill(&store, &skill_id)?;
        read_text_file_for_skill(&skill.id, Path::new(&skill.central_path), &relative_path)
    })
    .await?
}

#[tauri::command]
pub async fn preview_skill_file_save(
    skill_id: String,
    relative_path: String,
    updated_text: String,
    original_hash: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<SkillFileDiffDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = load_skill(&store, &skill_id)?;
        let current = read_text_file_for_skill(&skill.id, Path::new(&skill.central_path), &relative_path)?;
        if current.hash != original_hash {
            return Err(AppError::invalid_input(
                "File changed on disk. Reload before saving.",
            ));
        }
        Ok(SkillFileDiffDto {
            skill_id,
            relative_path,
            original_text: current.content,
            updated_text,
            original_hash,
        })
    })
    .await?
}

#[tauri::command]
pub async fn save_skill_file(
    skill_id: String,
    relative_path: String,
    updated_text: String,
    original_hash: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ManagedSkillDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        save_skill_file_unlocked(
            &store,
            &skill_id,
            &relative_path,
            &updated_text,
            &original_hash,
        )
    })
    .await?
}

fn save_skill_file_unlocked(
    store: &SkillStore,
    skill_id: &str,
    relative_path: &str,
    updated_text: &str,
    original_hash: &str,
) -> Result<ManagedSkillDto, AppError> {
    let _lock = RepoLock::acquire_foreground("edit skill file").map_err(AppError::db)?;
    let skill = load_skill(store, skill_id)?;
    let base = PathBuf::from(&skill.central_path);
    let current = read_text_file_for_skill(&skill.id, &base, relative_path)?;
    if current.hash != original_hash {
        return Err(AppError::invalid_input(
            "File changed on disk. Reload before saving.",
        ));
    }

    let path = resolve_skill_relative_path(&base, relative_path)?;
    let snapshot = create_skill_edit_snapshot(&skill, relative_path, current.content.as_bytes(), &current.hash)?;
    fs::write(&path, updated_text.as_bytes()).map_err(AppError::io)?;
    let content_hash = content_hash::hash_directory(&base).map_err(AppError::io)?;
    store
        .mark_skill_content_edited(skill_id, Some(&content_hash))
        .map_err(AppError::db)?;
    sync_existing_skill_targets(store, skill_id);
    let _ = store.log_audit(
        AuditDraft::new("edit")
            .skill(skill.id.clone(), skill.name.clone())
            .detail(format!("{}|snapshot:{}", relative_path, snapshot.id))
            .ok(),
    );
    sync_metadata::write_all_from_db_unlocked(store).map_err(AppError::db)?;

    let refreshed = load_skill(store, skill_id)?;
    let targets = store.get_all_targets().map_err(AppError::db)?;
    let tags_map = store.get_tags_map().map_err(AppError::db)?;
    Ok(managed_skill_to_dto(store, refreshed, &targets, &tags_map))
}

#[tauri::command]
pub async fn list_skill_edit_snapshots(
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<SkillEditSnapshotDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = load_skill(&store, &skill_id)?;
        list_edit_snapshots_for_skill(&skill)
    })
    .await?
}

#[tauri::command]
pub async fn restore_skill_edit_snapshot(
    skill_id: String,
    snapshot_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ManagedSkillDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _lock = RepoLock::acquire_foreground("restore skill edit snapshot").map_err(AppError::db)?;
        let skill = load_skill(&store, &skill_id)?;
        let base = PathBuf::from(&skill.central_path);
        let (meta, content) = read_edit_snapshot(&skill, &snapshot_id)?;
        let current = read_text_file_for_skill(&skill.id, &base, &meta.relative_path)?;
        create_skill_edit_snapshot(&skill, &meta.relative_path, current.content.as_bytes(), &current.hash)?;
        let path = resolve_skill_relative_path(&base, &meta.relative_path)?;
        fs::write(&path, &content).map_err(AppError::io)?;
        let content_hash = content_hash::hash_directory(&base).map_err(AppError::io)?;
        store
            .mark_skill_content_edited(&skill_id, Some(&content_hash))
            .map_err(AppError::db)?;
        sync_existing_skill_targets(&store, &skill_id);
        let _ = store.log_audit(
            AuditDraft::new("rollback")
                .skill(skill.id.clone(), skill.name.clone())
                .detail(format!("{}|snapshot:{}", meta.relative_path, snapshot_id))
                .ok(),
        );
        sync_metadata::write_all_from_db_unlocked(&store).map_err(AppError::db)?;

        let refreshed = load_skill(&store, &skill_id)?;
        let targets = store.get_all_targets().map_err(AppError::db)?;
        let tags_map = store.get_tags_map().map_err(AppError::db)?;
        Ok(managed_skill_to_dto(&store, refreshed, &targets, &tags_map))
    })
    .await?
}

#[tauri::command]
pub async fn preview_skillssh_skill(
    source: String,
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<MarketSkillPreviewDto, AppError> {
    let store = store.inner().clone();
    let proxy_url = store.proxy_url();
    tauri::async_runtime::spawn_blocking(move || {
        let repo_url = format!("https://github.com/{}.git", source);
        git_fetcher::validate_git_url(&repo_url).map_err(AppError::git)?;
        let temp_dir = git_fetcher::clone_repo_ref(&repo_url, None, None, proxy_url.as_deref())
            .map_err(AppError::classify_git_error)?;

        let result = (|| -> Result<MarketSkillPreviewDto, AppError> {
            let skill_dir = resolve_skill_dir(&temp_dir, None, Some(&skill_id))?;
            build_market_skill_preview_from_dir(&source, &skill_id, &skill_dir)
        })();

        git_fetcher::cleanup_temp(&temp_dir);
        result
    })
    .await?
}

fn build_market_skill_preview_from_dir(
    source: &str,
    skill_id: &str,
    skill_dir: &Path,
) -> Result<MarketSkillPreviewDto, AppError> {
    let meta = skill_metadata::parse_skill_md(skill_dir);
    let files = build_skill_file_tree(skill_dir)?;
    let document = read_skill_document_from_dir(skill_dir)
        .ok()
        .and_then(|(filename, _)| {
            read_text_file_for_skill(&format!("{source}/{skill_id}"), skill_dir, &filename).ok()
        });
    let file_previews = collect_market_file_previews(source, skill_id, skill_dir)?;
    let risk_issues = check_quality_for_dir(skill_dir)?;
    Ok(MarketSkillPreviewDto {
        source: source.to_string(),
        skill_id: skill_id.to_string(),
        name: meta.name.unwrap_or_else(|| skill_id.to_string()),
        description: meta.description,
        files,
        document,
        file_previews,
        risk_issues,
    })
}

#[tauri::command]
pub async fn check_skill_quality(
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<SkillQualityIssueDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = load_skill(&store, &skill_id)?;
        check_quality_for_dir(Path::new(&skill.central_path))
    })
    .await?
}

#[tauri::command]
pub async fn export_skill_archive(
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<String, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = load_skill(&store, &skill_id)?;
        let base = PathBuf::from(&skill.central_path);
        let export_dir = central_repo::base_dir().join("exports");
        fs::create_dir_all(&export_dir).map_err(AppError::io)?;
        let file_name = format!(
            "{}.zip",
            skill_metadata::sanitize_skill_name(&skill.name).unwrap_or_else(|| skill.id.clone())
        );
        let target = export_dir.join(file_name);
        write_skill_zip(&base, &target)?;
        store.log_audit(
            AuditDraft::new("export")
                .skill(skill.id.clone(), skill.name.clone())
                .detail(target.to_string_lossy().to_string())
                .ok(),
        );
        Ok(target.to_string_lossy().to_string())
    })
    .await?
}

#[tauri::command]
pub async fn get_skill_audit_history(
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<SkillAuditEntryDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let rows = store.list_audit(Some(200)).map_err(AppError::db)?;
        Ok(rows
            .into_iter()
            .filter(|entry| entry.skill_id.as_deref() == Some(skill_id.as_str()))
            .map(audit_entry_to_dto)
            .collect())
    })
    .await?
}

#[tauri::command]
pub async fn get_source_skill_document(
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<SourceSkillDocumentDto, AppError> {
    let store = store.inner().clone();
    let proxy_url = store.proxy_url();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = store
            .get_skill_by_id(&skill_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Skill not found"))?;

        if matches!(skill.source_type.as_str(), "local" | "import") {
            let source_path = skill.source_ref.as_ref().ok_or_else(|| {
                AppError::not_found("Local skill is missing its original source path")
            })?;
            let source_dir = PathBuf::from(source_path);
            if !source_dir.exists() {
                return Err(AppError::not_found("Original source path no longer exists"));
            }
            let (filename, content) = read_skill_document_from_dir(&source_dir)?;
            return Ok(SourceSkillDocumentDto {
                skill_id,
                filename,
                content,
                source_label: source_label_for_skill(&skill),
                revision: "workspace".to_string(),
            });
        }

        if !matches!(skill.source_type.as_str(), "git" | "skillssh") {
            return Err(AppError::invalid_input(
                "Skill does not support source diff preview",
            ));
        }

        let git_source = git_source_from_skill(&skill)?;
        git_fetcher::validate_git_url(&git_source.clone_url).map_err(AppError::git)?;
        let remote_revision = git_fetcher::resolve_remote_revision(
            &git_source.clone_url,
            git_source.branch.as_deref(),
            proxy_url.as_deref(),
        )
        .map_err(AppError::git)?;

        let temp_dir = git_fetcher::clone_repo_ref(
            &git_source.clone_url,
            git_source.branch.as_deref(),
            None,
            proxy_url.as_deref(),
        )
        .map_err(AppError::classify_git_error)?;

        let result = (|| -> Result<SourceSkillDocumentDto, AppError> {
            git_fetcher::checkout_revision(&temp_dir, &remote_revision).map_err(AppError::git)?;
            let skill_dir = resolve_skill_dir(
                &temp_dir,
                git_source.subpath.as_deref(),
                git_source.locator_skill_id.as_deref(),
            )?;
            let (filename, content) = read_skill_document_from_dir(&skill_dir)?;

            Ok(SourceSkillDocumentDto {
                skill_id,
                filename,
                content,
                source_label: source_label_for_skill(&skill),
                revision: remote_revision,
            })
        })();

        git_fetcher::cleanup_temp(&temp_dir);
        result
    })
    .await?
}

/// Files larger than this are flagged but not sent to the frontend — the
/// line diff is O(n²), so previewing a huge file would hang the UI.
const MAX_DIFF_FILE_BYTES: usize = 256 * 1024;

/// Classify a file's bytes for diffing: oversized and binary files get a
/// summary row instead of a text body.
fn classify_diff_bytes(bytes: Option<Vec<u8>>) -> (&'static str, Option<String>) {
    match bytes {
        Some(b) if b.len() > MAX_DIFF_FILE_BYTES => ("too_large", None),
        Some(b) if b.contains(&0) => ("binary", None),
        Some(b) => match String::from_utf8(b) {
            Ok(text) => ("text", Some(text)),
            Err(_) => ("binary", None),
        },
        None => ("binary", None),
    }
}

/// Diff the whole content scope of two skill directories. `original_dir` is
/// the central copy (old), `updated_dir` is the source (new). Uses the same
/// file enumeration as the hash so it reports exactly what flips the badge.
fn build_source_diff_entries(original_dir: &Path, updated_dir: &Path) -> Vec<SkillSourceDiffEntryDto> {
    use std::collections::BTreeMap;
    use crate::core::content_hash::{self, ContentEntry};

    let index = |dir: &Path| -> BTreeMap<String, ContentEntry> {
        content_hash::list_content_files(dir)
            .into_iter()
            .map(|e| (e.relative_path.clone(), e))
            .collect()
    };
    let original = index(original_dir);
    let updated = index(updated_dir);

    let mut keys: Vec<&String> = original.keys().chain(updated.keys()).collect();
    keys.sort();
    keys.dedup();

    let mut entries = Vec::new();
    for key in keys {
        match (original.get(key), updated.get(key)) {
            (None, Some(u)) => {
                let (kind, text) = classify_diff_bytes(std::fs::read(&u.path).ok());
                entries.push(SkillSourceDiffEntryDto {
                    relative_path: key.clone(),
                    status: "added".into(),
                    content_kind: kind.into(),
                    original_text: None,
                    updated_text: text,
                    executable_before: false,
                    executable_after: u.is_executable(),
                });
            }
            (Some(o), None) => {
                let (kind, text) = classify_diff_bytes(std::fs::read(&o.path).ok());
                entries.push(SkillSourceDiffEntryDto {
                    relative_path: key.clone(),
                    status: "removed".into(),
                    content_kind: kind.into(),
                    original_text: text,
                    updated_text: None,
                    executable_before: o.is_executable(),
                    executable_after: false,
                });
            }
            (Some(o), Some(u)) => {
                let o_bytes = std::fs::read(&o.path).ok();
                let u_bytes = std::fs::read(&u.path).ok();
                let exec_before = o.is_executable();
                let exec_after = u.is_executable();
                let bytes_equal = o_bytes.is_some() && o_bytes == u_bytes;

                if bytes_equal {
                    if exec_before == exec_after {
                        continue; // unchanged — must match the hash's verdict
                    }
                    entries.push(SkillSourceDiffEntryDto {
                        relative_path: key.clone(),
                        status: "modified".into(),
                        content_kind: "permission_only".into(),
                        original_text: None,
                        updated_text: None,
                        executable_before: exec_before,
                        executable_after: exec_after,
                    });
                    continue;
                }

                let (o_kind, o_text) = classify_diff_bytes(o_bytes);
                let (u_kind, u_text) = classify_diff_bytes(u_bytes);
                let (kind, original_text, updated_text) = if o_kind == "text" && u_kind == "text" {
                    ("text", o_text, u_text)
                } else if o_kind == "too_large" || u_kind == "too_large" {
                    ("too_large", None, None)
                } else {
                    ("binary", None, None)
                };
                entries.push(SkillSourceDiffEntryDto {
                    relative_path: key.clone(),
                    status: "modified".into(),
                    content_kind: kind.into(),
                    original_text,
                    updated_text,
                    executable_before: exec_before,
                    executable_after: exec_after,
                });
            }
            (None, None) => {}
        }
    }

    entries
}

#[tauri::command]
pub async fn get_skill_source_diff(
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<SkillSourceDiffDto, AppError> {
    let store = store.inner().clone();
    let proxy_url = store.proxy_url();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = store
            .get_skill_by_id(&skill_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Skill not found"))?;

        let central_dir = PathBuf::from(&skill.central_path);
        let source_label = source_label_for_skill(&skill);

        if matches!(skill.source_type.as_str(), "local" | "import") {
            let source_path = skill.source_ref.as_ref().ok_or_else(|| {
                AppError::not_found("Local skill is missing its original source path")
            })?;
            let source_dir = PathBuf::from(source_path);
            if !source_dir.exists() {
                return Err(AppError::not_found("Original source path no longer exists"));
            }
            let entries = build_source_diff_entries(&central_dir, &source_dir);
            return Ok(SkillSourceDiffDto {
                skill_id,
                source_label,
                revision: "workspace".to_string(),
                entries,
            });
        }

        if !matches!(skill.source_type.as_str(), "git" | "skillssh") {
            return Err(AppError::invalid_input(
                "Skill does not support source diff preview",
            ));
        }

        let git_source = git_source_from_skill(&skill)?;
        git_fetcher::validate_git_url(&git_source.clone_url).map_err(AppError::git)?;
        let remote_revision = git_fetcher::resolve_remote_revision(
            &git_source.clone_url,
            git_source.branch.as_deref(),
            proxy_url.as_deref(),
        )
        .map_err(AppError::git)?;

        let temp_dir = git_fetcher::clone_repo_ref(
            &git_source.clone_url,
            git_source.branch.as_deref(),
            None,
            proxy_url.as_deref(),
        )
        .map_err(AppError::classify_git_error)?;

        let result = (|| -> Result<SkillSourceDiffDto, AppError> {
            git_fetcher::checkout_revision(&temp_dir, &remote_revision).map_err(AppError::git)?;
            let skill_dir = resolve_skill_dir(
                &temp_dir,
                git_source.subpath.as_deref(),
                git_source.locator_skill_id.as_deref(),
            )?;
            let entries = build_source_diff_entries(&central_dir, &skill_dir);
            Ok(SkillSourceDiffDto {
                skill_id,
                source_label,
                revision: remote_revision,
                entries,
            })
        })();

        git_fetcher::cleanup_temp(&temp_dir);
        result
    })
    .await?
}

fn read_skill_document_from_dir(dir: &Path) -> Result<(String, String), AppError> {
    let candidates = [
        "SKILL.md",
        "skill.md",
        "CLAUDE.md",
        "claude.md",
        "README.md",
        "readme.md",
    ];

    for name in &candidates {
        let path = dir.join(name);
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            return Ok((name.to_string(), content));
        }
    }

    for e in WalkDir::new(dir).max_depth(4).into_iter().flatten() {
        let fname = e.file_name().to_string_lossy();
        if candidates.contains(&fname.as_ref()) {
            let content = std::fs::read_to_string(e.path())?;
            return Ok((fname.to_string(), content));
        }
    }

    Err(AppError::not_found("No documentation file found"))
}

fn load_skill(store: &SkillStore, skill_id: &str) -> Result<SkillRecord, AppError> {
    store
        .get_skill_by_id(skill_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Skill not found"))
}

fn normalize_relative_path(relative_path: &str) -> Result<PathBuf, AppError> {
    let raw = Path::new(relative_path);
    if raw.is_absolute() {
        return Err(AppError::invalid_input("Absolute paths are not allowed"));
    }

    let mut normalized = PathBuf::new();
    for component in raw.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(AppError::invalid_input("Path traversal is not allowed"));
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        return Err(AppError::invalid_input("File path is required"));
    }

    Ok(normalized)
}

fn resolve_skill_relative_path(base: &Path, relative_path: &str) -> Result<PathBuf, AppError> {
    let base = base.canonicalize().map_err(AppError::io)?;
    let normalized = normalize_relative_path(relative_path)?;
    let target = base.join(normalized);
    let parent = target
        .parent()
        .ok_or_else(|| AppError::invalid_input("Invalid file path"))?;
    let parent = parent.canonicalize().map_err(AppError::io)?;
    if !parent.starts_with(&base) {
        return Err(AppError::invalid_input("Path escapes the skill directory"));
    }
    Ok(target)
}

fn relative_path_from(base: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(base).unwrap_or(path).to_string_lossy().into_owned();
    #[cfg(windows)]
    let rel = rel.replace('\\', "/");
    rel
}

fn edit_history_dir(skill_id: &str) -> PathBuf {
    central_repo::base_dir().join("edit-history").join(skill_id)
}

fn create_skill_edit_snapshot(
    skill: &SkillRecord,
    relative_path: &str,
    content: &[u8],
    original_hash: &str,
) -> Result<SkillEditSnapshotMeta, AppError> {
    normalize_relative_path(relative_path)?;
    let ts = chrono::Utc::now().timestamp_millis();
    let id = format!("{}-{}", ts, uuid::Uuid::new_v4());
    let dir = edit_history_dir(&skill.id).join(&id);
    fs::create_dir_all(&dir).map_err(AppError::io)?;
    let meta = SkillEditSnapshotMeta {
        id,
        ts,
        skill_id: skill.id.clone(),
        skill_name: skill.name.clone(),
        relative_path: relative_path.to_string(),
        original_hash: original_hash.to_string(),
        size: content.len() as u64,
    };
    let meta_json = serde_json::to_vec_pretty(&meta).map_err(AppError::db)?;
    fs::write(dir.join("meta.json"), meta_json).map_err(AppError::io)?;
    fs::write(dir.join("content.txt"), content).map_err(AppError::io)?;
    Ok(meta)
}

fn read_edit_snapshot(
    skill: &SkillRecord,
    snapshot_id: &str,
) -> Result<(SkillEditSnapshotMeta, Vec<u8>), AppError> {
    normalize_relative_path(snapshot_id)?;
    let dir = edit_history_dir(&skill.id).join(snapshot_id);
    let meta_bytes = fs::read(dir.join("meta.json")).map_err(AppError::io)?;
    let meta: SkillEditSnapshotMeta = serde_json::from_slice(&meta_bytes).map_err(AppError::db)?;
    if meta.skill_id != skill.id {
        return Err(AppError::invalid_input("Snapshot does not belong to this skill"));
    }
    normalize_relative_path(&meta.relative_path)?;
    let content = fs::read(dir.join("content.txt")).map_err(AppError::io)?;
    Ok((meta, content))
}

fn list_edit_snapshots_for_skill(skill: &SkillRecord) -> Result<Vec<SkillEditSnapshotDto>, AppError> {
    let dir = edit_history_dir(&skill.id);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut snapshots = Vec::new();
    for entry in fs::read_dir(dir).map_err(AppError::io)? {
        let Ok(entry) = entry else {
            continue;
        };
        if !entry.path().is_dir() {
            continue;
        }
        let Ok(meta_bytes) = fs::read(entry.path().join("meta.json")) else {
            continue;
        };
        let Ok(meta) = serde_json::from_slice::<SkillEditSnapshotMeta>(&meta_bytes) else {
            continue;
        };
        if meta.skill_id != skill.id {
            continue;
        }
        snapshots.push(SkillEditSnapshotDto {
            id: meta.id,
            ts: meta.ts,
            skill_id: meta.skill_id,
            skill_name: meta.skill_name,
            relative_path: meta.relative_path,
            original_hash: meta.original_hash,
            size: meta.size,
        });
    }
    snapshots.sort_by(|a, b| b.ts.cmp(&a.ts));
    Ok(snapshots)
}

fn sync_existing_skill_targets(store: &SkillStore, skill_id: &str) {
    let targets = match store.get_targets_for_skill(skill_id) {
        Ok(targets) => targets,
        Err(err) => {
            log::warn!("Failed to list sync targets for edited skill {skill_id}: {err}");
            return;
        }
    };

    for target in targets {
        if let Err(err) = scenario_service::sync_single_skill_to_tool(store, skill_id, &target.tool) {
            log::warn!(
                "Failed to sync edited skill {} to {}: {}",
                skill_id,
                target.tool,
                err
            );
            let _ = store.log_audit(
                AuditDraft::new("sync")
                    .skill(skill_id.to_string(), String::new())
                    .tool(target.tool)
                    .fail(err.to_string()),
            );
        }
    }
}

fn modified_ms(meta: &fs::Metadata) -> Option<i64> {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(4096).any(|b| *b == 0)
}

fn is_probably_text_file(path: &Path, bytes: &[u8]) -> bool {
    if looks_binary(bytes) {
        return false;
    }
    if std::str::from_utf8(bytes).is_ok() {
        return true;
    }
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some(
            "md" | "markdown" | "txt" | "json" | "jsonc" | "yaml" | "yml" | "toml" | "rs"
                | "ts" | "tsx" | "js" | "jsx" | "css" | "html" | "xml" | "py" | "sh" | "bat"
                | "ps1" | "ini" | "conf" | "csv" | "gitignore"
        )
    )
}

fn read_text_file_for_skill(
    skill_id: &str,
    base: &Path,
    relative_path: &str,
) -> Result<SkillFileContentDto, AppError> {
    let path = resolve_skill_relative_path(base, relative_path)?;
    if !path.exists() {
        return Err(AppError::not_found("File not found"));
    }
    if !path.is_file() {
        return Err(AppError::invalid_input("Path is not a file"));
    }
    let meta = path.metadata().map_err(AppError::io)?;
    if meta.len() > MAX_EDITABLE_FILE_BYTES {
        return Err(AppError::invalid_input("File is too large to preview or edit"));
    }
    let bytes = fs::read(&path).map_err(AppError::io)?;
    if !is_probably_text_file(&path, &bytes) {
        return Err(AppError::invalid_input("Binary files cannot be previewed or edited"));
    }
    let content = String::from_utf8(bytes.clone())
        .map_err(|_| AppError::invalid_input("File is not valid UTF-8 text"))?;
    Ok(SkillFileContentDto {
        skill_id: skill_id.to_string(),
        relative_path: relative_path_from(&base.canonicalize().map_err(AppError::io)?, &path),
        content,
        size: meta.len(),
        modified_at: modified_ms(&meta),
        hash: hash_bytes(&bytes),
    })
}

fn collect_market_file_previews(
    source: &str,
    skill_id: &str,
    base: &Path,
) -> Result<Vec<MarketSkillFilePreviewDto>, AppError> {
    let base = base.canonicalize().map_err(AppError::io)?;
    let mut previews = Vec::new();
    let mut used_bytes = 0_u64;
    let preview_skill_id = format!("{source}/{skill_id}");

    for entry in WalkDir::new(&base)
        .max_depth(MAX_FILE_TREE_DEPTH)
        .into_iter()
        .flatten()
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.components().any(|component| {
            component
                .as_os_str()
                .to_str()
                .is_some_and(|part| matches!(part, ".git" | "__pycache__"))
        }) {
            continue;
        }

        let Ok(meta) = path.metadata() else {
            continue;
        };
        if meta.len() > MAX_EDITABLE_FILE_BYTES || used_bytes + meta.len() > MAX_MARKET_PREVIEW_BYTES {
            continue;
        }

        let relative_path = relative_path_from(&base, path);
        let Ok(content) = read_text_file_for_skill(&preview_skill_id, &base, &relative_path) else {
            continue;
        };
        used_bytes += content.size;
        previews.push(MarketSkillFilePreviewDto {
            relative_path: content.relative_path,
            content: content.content,
            size: content.size,
            hash: content.hash,
        });
    }

    previews.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(previews)
}

fn build_skill_file_tree(base: &Path) -> Result<Vec<SkillFileNodeDto>, AppError> {
    if !base.exists() {
        return Err(AppError::not_found("Skill directory not found"));
    }
    build_file_tree_level(base, base, 0)
}

fn build_file_tree_level(
    base: &Path,
    dir: &Path,
    depth: usize,
) -> Result<Vec<SkillFileNodeDto>, AppError> {
    if depth > MAX_FILE_TREE_DEPTH {
        return Ok(Vec::new());
    }
    let mut nodes = Vec::new();
    let entries = fs::read_dir(dir).map_err(AppError::io)?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if matches!(name.as_str(), ".git" | ".DS_Store" | "Thumbs.db" | "__pycache__") {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        let relative_path = relative_path_from(base, &path);
        if meta.is_dir() {
            nodes.push(SkillFileNodeDto {
                name,
                relative_path,
                kind: "directory".to_string(),
                size: None,
                modified_at: modified_ms(&meta),
                children: Some(build_file_tree_level(base, &path, depth + 1)?),
            });
        } else if meta.is_file() {
            nodes.push(SkillFileNodeDto {
                name,
                relative_path,
                kind: "file".to_string(),
                size: Some(meta.len()),
                modified_at: modified_ms(&meta),
                children: None,
            });
        }
    }
    nodes.sort_by(|a, b| match (a.kind.as_str(), b.kind.as_str()) {
        ("directory", "file") => std::cmp::Ordering::Less,
        ("file", "directory") => std::cmp::Ordering::Greater,
        _ => a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()),
    });
    Ok(nodes)
}

fn push_issue(
    issues: &mut Vec<SkillQualityIssueDto>,
    severity: &str,
    code: &str,
    message: &str,
    relative_path: Option<String>,
    line: Option<usize>,
) {
    issues.push(SkillQualityIssueDto {
        severity: severity.to_string(),
        code: code.to_string(),
        message: message.to_string(),
        relative_path,
        line,
    });
}

fn check_quality_for_dir(dir: &Path) -> Result<Vec<SkillQualityIssueDto>, AppError> {
    let mut issues = Vec::new();
    let doc = read_skill_document_from_dir(dir).ok();
    if doc.is_none() {
        push_issue(
            &mut issues,
            "error",
            "missing_document",
            "No SKILL.md, skill.md, CLAUDE.md, or README.md found.",
            None,
            None,
        );
    }

    let meta = skill_metadata::parse_skill_md(dir);
    if meta.name.as_deref().unwrap_or("").trim().is_empty() {
        push_issue(
            &mut issues,
            "warning",
            "missing_name",
            "SKILL.md frontmatter should include a name.",
            Some("SKILL.md".to_string()),
            None,
        );
    }
    if meta.description.as_deref().unwrap_or("").trim().is_empty() {
        push_issue(
            &mut issues,
            "info",
            "missing_description",
            "A short description helps users understand when to use this skill.",
            Some("SKILL.md".to_string()),
            None,
        );
    }

    let dangerous = [
        "rm -rf",
        "Remove-Item",
        "git reset --hard",
        "curl ",
        "Invoke-WebRequest",
        "sudo ",
    ];
    for entry in content_hash::list_content_files(dir) {
        let meta = match entry.path.metadata() {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        if meta.len() > MAX_EDITABLE_FILE_BYTES {
            push_issue(
                &mut issues,
                "warning",
                "large_file",
                "Large files slow down backup, review, and sync.",
                Some(entry.relative_path.clone()),
                None,
            );
            continue;
        }
        let Ok(bytes) = fs::read(&entry.path) else {
            continue;
        };
        if !is_probably_text_file(&entry.path, &bytes) {
            push_issue(
                &mut issues,
                "info",
                "binary_file",
                "Binary files cannot be reviewed inline.",
                Some(entry.relative_path.clone()),
                None,
            );
            continue;
        }
        if let Ok(content) = String::from_utf8(bytes) {
            for (index, line) in content.lines().enumerate() {
                if dangerous.iter().any(|needle| line.contains(needle)) {
                    push_issue(
                        &mut issues,
                        "warning",
                        "dangerous_command",
                        "This line contains a command pattern that deserves review.",
                        Some(entry.relative_path.clone()),
                        Some(index + 1),
                    );
                }
                for target in markdown_local_links(line) {
                    let target_path = target.split('#').next().unwrap_or("").trim();
                    if target_path.is_empty() {
                        continue;
                    }
                    let Some(parent) = entry.path.parent() else {
                        continue;
                    };
                    if !parent.join(target_path).exists() {
                        push_issue(
                            &mut issues,
                            "warning",
                            "missing_reference",
                            "This local reference points to a missing file.",
                            Some(entry.relative_path.clone()),
                            Some(index + 1),
                        );
                    }
                }
            }
        }
    }
    Ok(issues)
}

fn markdown_local_links(line: &str) -> Vec<&str> {
    let mut links = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find("](") {
        let after = &rest[start + 2..];
        let Some(end) = after.find(')') else {
            break;
        };
        let target = after[..end].trim();
        let lower = target.to_ascii_lowercase();
        if !target.is_empty()
            && !target.starts_with('#')
            && !lower.starts_with("http://")
            && !lower.starts_with("https://")
            && !lower.starts_with("mailto:")
            && !lower.starts_with("data:")
        {
            links.push(target);
        }
        rest = &after[end + 1..];
    }
    links
}

fn write_skill_zip(base: &Path, target: &Path) -> Result<(), AppError> {
    let file = File::create(target).map_err(AppError::io)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::FileOptions::<()>::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut buffer = Vec::new();

    for entry in content_hash::list_content_files(base) {
        buffer.clear();
        let mut input = File::open(&entry.path).map_err(AppError::io)?;
        input.read_to_end(&mut buffer).map_err(AppError::io)?;
        zip.start_file(entry.relative_path, options)
            .map_err(AppError::io)?;
        zip.write_all(&buffer).map_err(AppError::io)?;
    }

    zip.finish().map_err(AppError::io)?;
    Ok(())
}

fn audit_entry_to_dto(entry: AuditEntry) -> SkillAuditEntryDto {
    SkillAuditEntryDto {
        id: entry.id,
        ts: entry.ts,
        action: entry.action,
        tool: entry.tool,
        success: entry.success,
        detail: entry.detail,
    }
}

fn source_label_for_skill(skill: &SkillRecord) -> String {
    match skill.source_type.as_str() {
        "skillssh" => "skills.sh".to_string(),
        "git" => "Git".to_string(),
        "local" => "Local".to_string(),
        "import" => "Imported".to_string(),
        other => other.to_string(),
    }
}

#[tauri::command]
pub async fn delete_managed_skill(
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = delete_managed_skills_by_ids(&store, &[skill_id.clone()])?;
        if result.deleted == 0 {
            return Err(AppError::not_found("Skill not found"));
        }
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn delete_managed_skills(
    skill_ids: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<BatchDeleteSkillsResult, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || delete_managed_skills_by_ids(&store, &skill_ids))
        .await?
}

pub fn delete_managed_skills_by_ids(
    store: &SkillStore,
    skill_ids: &[String],
) -> Result<BatchDeleteSkillsResult, AppError> {
    sync_metadata::with_repo_lock("delete skills", || {
        let mut deleted = 0;
        let mut failed = Vec::new();

        for skill_id in skill_ids {
            let Some(skill) = store.get_skill_by_id(skill_id)? else {
                store.log_audit(
                    AuditDraft::new("remove")
                        .skill(skill_id.clone(), "")
                        .fail("not found"),
                );
                failed.push(skill_id.clone());
                continue;
            };

            let targets = store.get_targets_for_skill(skill_id)?;
            for target in &targets {
                let target_path = PathBuf::from(&target.target_path);
                sync_engine::remove_target(&target_path).ok();
            }

            let central = PathBuf::from(&skill.central_path);
            if central.exists() {
                std::fs::remove_dir_all(&central).ok();
            }

            store.delete_skill(skill_id)?;
            store.log_audit(
                AuditDraft::new("remove")
                    .skill(skill_id.clone(), skill.name.clone())
                    .ok(),
            );
            deleted += 1;
        }

        if deleted > 0 {
            sync_metadata::write_all_from_db_unlocked(store)?;
        }

        Ok(BatchDeleteSkillsResult { deleted, failed })
    })
    .map_err(AppError::db)
}

/// Append an audit log entry summarising an install attempt.
/// `source_label` is short text identifying the source (e.g. "local", "git", "skillssh").
fn log_install_outcome(
    store: &SkillStore,
    source_label: &str,
    outcome: Result<&(String, String), &AppError>,
) {
    let draft = AuditDraft::new("install").detail(source_label);
    let draft = match outcome {
        Ok((id, name)) => draft.skill(id.clone(), name.clone()).ok(),
        Err(e) => draft.fail(e.to_string()),
    };
    store.log_audit(draft);
}

fn log_update_outcome(
    store: &SkillStore,
    skill_id: &str,
    source_label: &str,
    outcome: Result<&UpdateSkillResult, &AppError>,
) {
    let mut draft = AuditDraft::new("update").detail(source_label);
    match outcome {
        Ok(result) => {
            draft = draft
                .skill(result.skill.id.clone(), result.skill.name.clone())
                .detail(if result.content_changed {
                    format!("{source_label}; content changed")
                } else {
                    format!("{source_label}; unchanged")
                })
                .ok();
        }
        Err(e) => {
            let name = store
                .get_skill_by_id(skill_id)
                .ok()
                .flatten()
                .map(|s| s.name)
                .unwrap_or_default();
            draft = draft.skill(skill_id.to_string(), name).fail(e.to_string());
        }
    }
    store.log_audit(draft);
}

fn log_reimport_outcome(
    store: &SkillStore,
    skill_id: &str,
    outcome: Result<&ManagedSkillDto, &AppError>,
) {
    let mut draft = AuditDraft::new("update").detail("local");
    match outcome {
        Ok(dto) => {
            draft = draft.skill(dto.id.clone(), dto.name.clone()).ok();
        }
        Err(e) => {
            let name = store
                .get_skill_by_id(skill_id)
                .ok()
                .flatten()
                .map(|s| s.name)
                .unwrap_or_default();
            draft = draft.skill(skill_id.to_string(), name).fail(e.to_string());
        }
    }
    store.log_audit(draft);
}

#[tauri::command]
pub async fn install_local(
    source_path: String,
    name: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let outcome = (|| -> Result<(String, String), AppError> {
            let path = PathBuf::from(&source_path);
            let metadata = InstallSourceMetadata {
                source_type: "local".to_string(),
                source_ref: Some(source_path.clone()),
                source_ref_resolved: None,
                source_subpath: None,
                source_branch: None,
                source_revision: None,
                remote_revision: None,
                update_status: "local_only".to_string(),
            };
            let _lock = RepoLock::acquire_foreground("install local skill").map_err(AppError::db)?;
            let result =
                installer::install_from_local(&path, name.as_deref()).map_err(AppError::io)?;
            let skill_name = result.name.clone();
            // Install only adds the skill to the central library; preset
            // membership is an explicit action (see issue #213).
            let skill_id =
                store_installed_skill_unlocked(&store, &result, &metadata, None)?;
            Ok((skill_id, skill_name))
        })();
        log_install_outcome(&store, "local", outcome.as_ref());
        outcome.map(|_| ())
    })
    .await?
}

#[tauri::command]
pub async fn install_git(
    repo_url: String,
    name: Option<String>,
    store: State<'_, Arc<SkillStore>>,
    cancel_registry: State<'_, Arc<InstallCancelRegistry>>,
    app_handle: tauri::AppHandle,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let proxy_url = store.proxy_url();
    let registry = cancel_registry.inner().clone();
    let cancel_key = repo_url.clone();
    let cancel = registry.register(&cancel_key);
    let _cancel_guard = CancelRegistrationGuard::new(registry.clone(), cancel_key);

    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Emitter;
        let emit_progress = |phase: &str| {
            app_handle
                .emit(
                    "install-progress",
                    serde_json::json!({
                        "skill_id": repo_url,
                        "phase": phase,
                    }),
                )
                .ok();
        };

        let outcome = (|| -> Result<(String, String), AppError> {
            git_fetcher::validate_git_url(&repo_url).map_err(AppError::git)?;
            emit_progress("cloning");
            let parsed = git_fetcher::parse_git_source_resolved(&repo_url, proxy_url.as_deref());
            let app_for_progress = app_handle.clone();
            let url_for_progress = repo_url.clone();
            let progress_cb: git_fetcher::ProgressCallback = Box::new(move |msg: &str| {
                app_for_progress
                    .emit(
                        "install-progress",
                        serde_json::json!({
                            "skill_id": url_for_progress,
                            "phase": "cloning",
                            "detail": msg,
                        }),
                    )
                    .ok();
            });
            let temp_dir = git_fetcher::clone_repo_ref_with_progress(
                &parsed.clone_url,
                parsed.branch.as_deref(),
                Some(&cancel),
                proxy_url.as_deref(),
                Some(progress_cb),
            )
            .map_err(AppError::classify_git_error)?;

            emit_progress("installing");
            let install_result = (|| -> Result<(String, String), AppError> {
                let _lock = RepoLock::acquire_foreground("install git skill").map_err(AppError::db)?;
                let skill_dir = resolve_skill_dir(&temp_dir, parsed.subpath.as_deref(), None)?;
                let revision = git_fetcher::get_head_revision(&temp_dir).map_err(AppError::git)?;
                let result = installer::install_from_git_dir(&skill_dir, name.as_deref())
                    .map_err(AppError::io)?;
                let metadata = InstallSourceMetadata {
                    source_type: "git".to_string(),
                    source_ref: Some(parsed.original_url.clone()),
                    source_ref_resolved: Some(parsed.clone_url.clone()),
                    source_subpath: git_fetcher::relative_subpath(&temp_dir, &skill_dir),
                    source_branch: parsed.branch.clone(),
                    source_revision: Some(revision.clone()),
                    remote_revision: Some(revision),
                    update_status: "up_to_date".to_string(),
                };
                let skill_name = result.name.clone();
                let skill_id = store_installed_skill_unlocked(
                    &store,
                    &result,
                    &metadata,
                    None,
                )?;
                Ok((skill_id, skill_name))
            })();

            git_fetcher::cleanup_temp(&temp_dir);
            install_result
        })();

        log_install_outcome(&store, "git", outcome.as_ref());
        outcome?;

        emit_progress("done");
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn install_from_skillssh(
    source: String,
    skill_id: String,
    name: Option<String>,
    store: State<'_, Arc<SkillStore>>,
    cancel_registry: State<'_, Arc<InstallCancelRegistry>>,
    app_handle: tauri::AppHandle,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let proxy_url = store.proxy_url();
    let registry = cancel_registry.inner().clone();
    let cancel_key_owned = format!("{}/{}", source, skill_id);
    let cancel = registry.register(&cancel_key_owned);
    let _cancel_guard = CancelRegistrationGuard::new(registry.clone(), cancel_key_owned);

    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Emitter;
        let skill_key = format!("{}/{}", source, skill_id);
        let emit_progress = |phase: &str| {
            app_handle
                .emit(
                    "install-progress",
                    serde_json::json!({
                        "skill_id": skill_key,
                        "phase": phase,
                    }),
                )
                .ok();
        };

        let outcome = (|| -> Result<(String, String), AppError> {
            emit_progress("cloning");
            let repo_url = format!("https://github.com/{}.git", source);
            let app_for_progress = app_handle.clone();
            let skill_key_for_progress = skill_key.clone();
            let progress_cb: git_fetcher::ProgressCallback = Box::new(move |msg: &str| {
                app_for_progress
                    .emit(
                        "install-progress",
                        serde_json::json!({
                            "skill_id": skill_key_for_progress,
                            "phase": "cloning",
                            "detail": msg,
                        }),
                    )
                    .ok();
            });
            let temp_dir = git_fetcher::clone_repo_ref_with_progress(
                &repo_url,
                None,
                Some(&cancel),
                proxy_url.as_deref(),
                Some(progress_cb),
            )
            .map_err(AppError::classify_git_error)?;

            emit_progress("installing");
            let install_result = (|| -> Result<(String, String), AppError> {
                let _lock = RepoLock::acquire_foreground("install skillssh skill").map_err(AppError::db)?;
                let skill_dir = resolve_skill_dir(&temp_dir, None, Some(&skill_id))?;
                let revision = git_fetcher::get_head_revision(&temp_dir).map_err(AppError::git)?;
                let source_ref = format!("{}/{}", source, skill_id);
                let requested_name = name
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                let (install_name, destination) =
                    resolve_skillssh_install_target(&store, &source_ref, &skill_id, requested_name)?;
                let result = installer::install_skill_dir_to_destination(
                    &skill_dir,
                    &install_name,
                    &destination,
                )
                .map_err(AppError::io)?;
                let metadata = InstallSourceMetadata {
                    source_type: "skillssh".to_string(),
                    source_ref: Some(source_ref),
                    source_ref_resolved: Some(repo_url.clone()),
                    source_subpath: git_fetcher::relative_subpath(&temp_dir, &skill_dir),
                    source_branch: None,
                    source_revision: Some(revision.clone()),
                    remote_revision: Some(revision),
                    update_status: "up_to_date".to_string(),
                };
                let skill_name = result.name.clone();
                let new_id = store_installed_skill_unlocked(
                    &store,
                    &result,
                    &metadata,
                    None,
                )?;
                Ok((new_id, skill_name))
            })();

            git_fetcher::cleanup_temp(&temp_dir);
            install_result
        })();

        log_install_outcome(&store, "skillssh", outcome.as_ref());
        outcome?;

        emit_progress("done");
        Ok(())
    })
    .await?
}

/// Clone a git repo and return a preview list of skills found, without installing.
/// The caller must follow up with `confirm_git_install` using the returned `temp_dir`.
#[tauri::command]
pub async fn preview_git_install(
    repo_url: String,
    store: State<'_, Arc<SkillStore>>,
    cancel_registry: State<'_, Arc<InstallCancelRegistry>>,
    app_handle: tauri::AppHandle,
) -> Result<GitPreviewResult, AppError> {
    let store = store.inner().clone();
    let proxy_url = store.get_setting("proxy_url").ok().flatten();
    let registry = cancel_registry.inner().clone();
    let cancel_key = repo_url.clone();
    let cancel = registry.register(&cancel_key);
    let _cancel_guard = CancelRegistrationGuard::new(registry.clone(), cancel_key);

    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Emitter;
        app_handle
            .emit(
                "install-progress",
                serde_json::json!({
                    "skill_id": repo_url,
                    "phase": "cloning",
                }),
            )
            .ok();

        let parsed = git_fetcher::parse_git_source_resolved(&repo_url, proxy_url.as_deref());
        let app_for_progress = app_handle.clone();
        let url_for_progress = repo_url.clone();
        let progress_cb: git_fetcher::ProgressCallback = Box::new(move |msg: &str| {
            app_for_progress
                .emit(
                    "install-progress",
                    serde_json::json!({
                        "skill_id": url_for_progress,
                        "phase": "cloning",
                        "detail": msg,
                    }),
                )
                .ok();
        });
        let temp_dir = git_fetcher::clone_repo_ref_with_progress(
            &parsed.clone_url,
            parsed.branch.as_deref(),
            Some(&cancel),
            proxy_url.as_deref(),
            Some(progress_cb),
        )
        .map_err(AppError::classify_git_error)?;

        let build_preview = || -> Result<GitPreviewResult, AppError> {
            let skill_dir = resolve_skill_dir(&temp_dir, parsed.subpath.as_deref(), None)?;
            let dirs = collect_git_skill_dirs(&skill_dir);

            let skills: Vec<GitSkillPreview> = dirs
                .iter()
                .map(|dir| {
                    let meta = skill_metadata::parse_skill_md(dir);
                    let rel_path = skill_rel_key(&skill_dir, dir);
                    let basename = dir
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| rel_path.clone());
                    let name = meta
                        .name
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or_else(|| basename.clone());
                    GitSkillPreview {
                        rel_path,
                        name,
                        description: meta.description,
                    }
                })
                .collect();

            Ok(GitPreviewResult {
                temp_dir: temp_dir.to_string_lossy().to_string(),
                skills,
            })
        };

        build_preview().inspect_err(|_e| {
            git_fetcher::cleanup_temp(&temp_dir);
        })
    })
    .await?
}

/// Install selected skills from a previously cloned temp directory.
#[tauri::command]
pub async fn confirm_git_install(
    repo_url: String,
    temp_dir: String,
    items: Vec<SkillInstallItem>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let proxy_url = store.proxy_url();
    tauri::async_runtime::spawn_blocking(move || {
        let temp_path = validate_clone_temp_path(&temp_dir)?;

        let result: Result<(), AppError> = (|| {
            if items.is_empty() {
                return Ok(());
            }

            let parsed = git_fetcher::parse_git_source_resolved(&repo_url, proxy_url.as_deref());
            let skill_dir = resolve_skill_dir(&temp_path, parsed.subpath.as_deref(), None)?;
            let all_dirs = collect_git_skill_dirs(&skill_dir);
            let revision = git_fetcher::get_head_revision(&temp_path).map_err(AppError::git)?;
            let _lock = RepoLock::acquire_foreground("confirm git install")
                .map_err(AppError::db)?;

            for dir in &all_dirs {
                let rel_key = skill_rel_key(&skill_dir, dir);
                let item = match items.iter().find(|i| i.rel_path == rel_key) {
                    Some(i) => i,
                    None => continue,
                };
                let custom_name = item.name.trim();
                let install_name = if custom_name.is_empty() {
                    None
                } else {
                    Some(custom_name)
                };
                let result =
                    installer::install_from_git_dir(dir, install_name).map_err(AppError::io)?;
                let subpath = git_fetcher::relative_subpath(&temp_path, dir);
                let metadata = InstallSourceMetadata {
                    source_type: "git".to_string(),
                    source_ref: Some(repo_url.clone()),
                    source_ref_resolved: Some(parsed.clone_url.clone()),
                    source_subpath: subpath,
                    source_branch: parsed.branch.clone(),
                    source_revision: Some(revision.clone()),
                    remote_revision: Some(revision.clone()),
                    update_status: "up_to_date".to_string(),
                };
                store_installed_skill_unlocked(&store, &result, &metadata, None)?;
            }
            Ok(())
        })();

        // Always clean up temp directory, regardless of success or failure.
        git_fetcher::cleanup_temp(&temp_path);
        result
    })
    .await?
}

/// Clean up temp directory from a cancelled preview session.
#[tauri::command]
pub async fn cancel_git_preview(temp_dir: String) -> Result<(), AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        if let Ok(temp_path) = validate_clone_temp_path(&temp_dir) {
            git_fetcher::cleanup_temp(&temp_path);
        }
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn check_skill_update(
    skill_id: String,
    force: Option<bool>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ManagedSkillDto, AppError> {
    let store = store.inner().clone();
    let proxy_url = store.proxy_url();
    tauri::async_runtime::spawn_blocking(move || {
        let _lock = RepoLock::acquire_foreground("check skill update").map_err(AppError::db)?;
        check_skill_update_internal(
            &store,
            &skill_id,
            force.unwrap_or(false),
            proxy_url.as_deref(),
        )
    })
    .await?
}

#[tauri::command]
pub async fn check_all_skill_updates(
    force: Option<bool>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let proxy_url = store.proxy_url();
    tauri::async_runtime::spawn_blocking(move || {
        let force_check = force.unwrap_or(false);
        let ids: Vec<String> = store
            .get_all_skills()
            .map_err(AppError::db)?
            .into_iter()
            .map(|skill| skill.id)
            .collect();
        let mut failed = Vec::new();

        for skill_id in ids {
            // Take the central-repo lock per skill so a concurrent manual
            // install/update can't race the `update_status` write. Lock
            // contention is reported as a per-skill failure so the caller
            // knows the check didn't complete.
            let _lock = match RepoLock::acquire("check skill update") {
                Ok(lock) => lock,
                Err(err) => {
                    failed.push(format!("{skill_id}: {err}"));
                    continue;
                }
            };
            if let Err(err) =
                check_skill_update_internal(&store, &skill_id, force_check, proxy_url.as_deref())
            {
                failed.push(format!("{skill_id}: {err}"));
            }
        }

        if failed.is_empty() {
            Ok(())
        } else {
            Err(AppError::internal(format!(
                "Failed to check {} skill(s): {}",
                failed.len(),
                failed.join("; ")
            )))
        }
    })
    .await?
}

#[tauri::command]
pub async fn update_skill(
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
    cancel_registry: State<'_, Arc<InstallCancelRegistry>>,
) -> Result<UpdateSkillResult, AppError> {
    let store = store.inner().clone();
    let proxy_url = store.proxy_url();
    let registry = cancel_registry.inner().clone();
    let cancel_key = format!("update:{}", skill_id);
    let cancel = registry.register(&cancel_key);
    let _cancel_guard = CancelRegistrationGuard::new(registry.clone(), cancel_key);

    tauri::async_runtime::spawn_blocking(move || {
        let outcome =
            update_git_skill_internal(&store, &skill_id, proxy_url.as_deref(), Some(&cancel));
        log_update_outcome(&store, &skill_id, "git", outcome.as_ref());
        outcome
    })
    .await?
}

#[tauri::command]
pub async fn reimport_local_skill(
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ManagedSkillDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let outcome = reimport_local_skill_internal(&store, &skill_id);
        log_reimport_outcome(&store, &skill_id, outcome.as_ref());
        outcome
    })
    .await?
}

#[tauri::command]
pub async fn batch_update_skills(
    skill_ids: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<BatchUpdateSkillsResult, AppError> {
    let store = store.inner().clone();
    let proxy_url = store.proxy_url();
    tauri::async_runtime::spawn_blocking(move || {
        let mut refreshed = 0usize;
        let mut unchanged = 0usize;
        let mut failed = Vec::new();

        for skill_id in skill_ids {
            let skill = match store.get_skill_by_id(&skill_id).map_err(AppError::db)? {
                Some(skill) => skill,
                None => {
                    failed.push(format!("{skill_id}: Skill not found"));
                    continue;
                }
            };

            match skill.source_type.as_str() {
                "git" | "skillssh" => {
                    let outcome =
                        update_git_skill_internal(&store, &skill_id, proxy_url.as_deref(), None);
                    log_update_outcome(&store, &skill_id, "git", outcome.as_ref());
                    match outcome {
                        Ok(result) => {
                            if result.content_changed {
                                refreshed += 1;
                            } else {
                                unchanged += 1;
                            }
                        }
                        Err(err) => failed.push(format!("{}: {}", skill.name, err.message)),
                    }
                }
                "local" | "import" => {
                    let outcome = reimport_local_skill_internal(&store, &skill_id);
                    log_reimport_outcome(&store, &skill_id, outcome.as_ref());
                    match outcome {
                        Ok(_) => refreshed += 1,
                        Err(err) => failed.push(format!("{}: {}", skill.name, err.message)),
                    }
                }
                _ => failed.push(format!("{}: Source type cannot be refreshed", skill.name)),
            }
        }

        Ok(BatchUpdateSkillsResult {
            refreshed,
            unchanged,
            failed,
        })
    })
    .await?
}

#[tauri::command]
pub async fn relink_local_skill_source(
    skill_id: String,
    source_path: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ManagedSkillDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = store
            .get_skill_by_id(&skill_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Skill not found"))?;

        if !matches!(skill.source_type.as_str(), "local" | "import") {
            return Err(AppError::invalid_input(
                "Only local skills can relink source paths",
            ));
        }

        let path = PathBuf::from(&source_path);
        if !path.exists() {
            return Err(AppError::not_found("Selected source path does not exist"));
        }
        if !is_valid_skill_dir(&path) {
            return Err(AppError::invalid_input(
                "Selected source path is not a valid skill directory",
            ));
        }

        store
            .update_skill_update_status(&skill_id, "updating")
            .map_err(AppError::db)?;

        let result = (|| -> Result<(), AppError> {
            let _lock = RepoLock::acquire_foreground("relink local skill")
                .map_err(AppError::db)?;
            let staged_path = staged_path_for(&skill.central_path);
            let install_result = installer::install_from_local_to_destination(
                &path,
                Some(&skill.name),
                &staged_path,
            )
            .map_err(AppError::io)?;
            swap_skill_directory(&staged_path, Path::new(&skill.central_path))?;
            store
                .update_skill_after_reinstall(
                    &skill.id,
                    &skill.name,
                    install_result.description.as_deref(),
                    &skill.source_type,
                    Some(&source_path),
                    None,
                    None,
                    None,
                    None,
                    None,
                    Some(&install_result.content_hash),
                    "local_only",
                )
                .map_err(AppError::db)?;
            resync_copy_targets(&store, &skill.id)?;
            sync_metadata::write_all_from_db_unlocked(&store).map_err(AppError::db)?;
            Ok(())
        })();

        match result {
            Ok(()) => managed_skill_by_id(&store, &skill_id),
            Err(e) => {
                let _ = store.update_skill_check_state(&skill_id, None, "error", Some(&e.message));
                Err(e)
            }
        }
    })
    .await?
}

#[tauri::command]
pub async fn detach_local_skill_source(
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ManagedSkillDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = store
            .get_skill_by_id(&skill_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Skill not found"))?;

        if !matches!(skill.source_type.as_str(), "local" | "import") {
            return Err(AppError::invalid_input(
                "Only local skills can detach source paths",
            ));
        }

        {
            let _lock = RepoLock::acquire_foreground("detach local skill")
                .map_err(AppError::db)?;
            store
                .update_skill_after_reinstall(
                    &skill.id,
                    &skill.name,
                    skill.description.as_deref(),
                    &skill.source_type,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    skill.content_hash.as_deref(),
                    "local_only",
                )
                .map_err(AppError::db)?;
            sync_metadata::write_all_from_db_unlocked(&store).map_err(AppError::db)?;
        }

        managed_skill_by_id(&store, &skill_id)
    })
    .await?
}

fn managed_skill_to_dto(
    store: &SkillStore,
    skill: SkillRecord,
    all_targets: &[SkillTargetRecord],
    tags_map: &std::collections::HashMap<String, Vec<String>>,
) -> ManagedSkillDto {
    let targets = all_targets
        .iter()
        .filter(|target| target.skill_id == skill.id)
        .map(|target| TargetDto {
            id: target.id.clone(),
            skill_id: target.skill_id.clone(),
            tool: target.tool.clone(),
            target_path: target.target_path.clone(),
            mode: target.mode.clone(),
            status: target.status.clone(),
            synced_at: target.synced_at,
        })
        .collect();

    let preset_ids = store.get_scenarios_for_skill(&skill.id).unwrap_or_default();
    let tags = tags_map.get(&skill.id).cloned().unwrap_or_default();

    // Prefer description from SKILL.md so the list view reflects edits made
    // directly on disk (file watcher emits a change event; this read serves
    // the fresh value). Keep `name` on the DB value to avoid drift with
    // sync target directory names.
    let description = skill_metadata::parse_skill_md(Path::new(&skill.central_path))
        .description
        .filter(|s| !s.trim().is_empty())
        .or(skill.description);

    ManagedSkillDto {
        id: skill.id,
        name: skill.name,
        description,
        source_type: skill.source_type,
        source_ref: skill.source_ref,
        source_ref_resolved: skill.source_ref_resolved,
        source_subpath: skill.source_subpath,
        source_branch: skill.source_branch,
        source_revision: skill.source_revision,
        remote_revision: skill.remote_revision,
        update_status: skill.update_status,
        last_checked_at: skill.last_checked_at,
        last_check_error: skill.last_check_error,
        central_path: skill.central_path,
        enabled: skill.enabled,
        created_at: skill.created_at,
        updated_at: skill.updated_at,
        status: skill.status,
        targets,
        preset_ids,
        tags,
    }
}

pub fn managed_skill_by_id(store: &SkillStore, skill_id: &str) -> Result<ManagedSkillDto, AppError> {
    let skill = store
        .get_skill_by_id(skill_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Skill not found"))?;
    let all_targets = store.get_all_targets().map_err(AppError::db)?;
    let tags_map = store.get_tags_map().map_err(AppError::db)?;
    Ok(managed_skill_to_dto(store, skill, &all_targets, &tags_map))
}

pub fn update_git_skill_internal(
    store: &SkillStore,
    skill_id: &str,
    proxy_url: Option<&str>,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<UpdateSkillResult, AppError> {
    let skill = store
        .get_skill_by_id(skill_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Skill not found"))?;

    if !matches!(skill.source_type.as_str(), "git" | "skillssh") {
        return Err(AppError::invalid_input(
            "Only git-based skills can be updated",
        ));
    }

    let git_source = git_source_from_skill(&skill)?;
    git_fetcher::validate_git_url(&git_source.clone_url).map_err(AppError::git)?;
    let remote_revision = git_fetcher::resolve_remote_revision(
        &git_source.clone_url,
        git_source.branch.as_deref(),
        proxy_url,
    )
    .map_err(|e| {
        let message = e.to_string();
        let _ = store.update_skill_check_state(
            skill_id,
            skill.remote_revision.as_deref(),
            "error",
            Some(&message),
        );
        AppError::git(message)
    })?;

    store
        .update_skill_update_status(skill_id, "updating")
        .map_err(AppError::db)?;

    let temp_dir = git_fetcher::clone_repo_ref(
        &git_source.clone_url,
        git_source.branch.as_deref(),
        cancel,
        proxy_url,
    )
    .map_err(AppError::classify_git_error)?;
    let update_result = (|| -> Result<bool, AppError> {
        git_fetcher::checkout_revision(&temp_dir, &remote_revision).map_err(AppError::git)?;
        let skill_dir = resolve_skill_dir(
            &temp_dir,
            git_source.subpath.as_deref(),
            git_source.locator_skill_id.as_deref(),
        )?;

        let new_hash =
            crate::core::content_hash::hash_directory(&skill_dir).map_err(AppError::io)?;
        let content_changed = skill.content_hash.as_deref() != Some(new_hash.as_str());
        let source_subpath = git_fetcher::relative_subpath(&temp_dir, &skill_dir);
        let _lock = RepoLock::acquire_foreground("update installed skill")
            .map_err(AppError::db)?;

        if content_changed {
            let staged_path = staged_path_for(&skill.central_path);
            let install_result =
                installer::install_skill_dir_to_destination(&skill_dir, &skill.name, &staged_path)
                    .map_err(AppError::io)?;
            swap_skill_directory(&staged_path, Path::new(&skill.central_path))?;

            store
                .update_skill_source_metadata(
                    &skill.id,
                    Some(&git_source.clone_url),
                    source_subpath.as_deref(),
                    git_source.branch.as_deref(),
                    Some(&remote_revision),
                )
                .map_err(AppError::db)?;
            store
                .update_skill_after_install(
                    &skill.id,
                    &skill.name,
                    install_result.description.as_deref(),
                    Some(&remote_revision),
                    Some(&remote_revision),
                    Some(&install_result.content_hash),
                    "up_to_date",
                )
                .map_err(AppError::db)?;
            resync_copy_targets(store, &skill.id)?;
            sync_metadata::write_all_from_db_unlocked(store).map_err(AppError::db)?;
        } else {
            store
                .update_skill_source_metadata(
                    &skill.id,
                    Some(&git_source.clone_url),
                    source_subpath.as_deref(),
                    git_source.branch.as_deref(),
                    Some(&remote_revision),
                )
                .map_err(AppError::db)?;
            store
                .update_skill_check_state(&skill.id, Some(&remote_revision), "up_to_date", None)
                .map_err(AppError::db)?;
            resync_copy_targets(store, &skill.id)?;
            sync_metadata::write_all_from_db_unlocked(store).map_err(AppError::db)?;
        }
        Ok(content_changed)
    })();
    git_fetcher::cleanup_temp(&temp_dir);

    match update_result {
        Ok(content_changed) => {
            let skill = managed_skill_by_id(store, skill_id)?;
            Ok(UpdateSkillResult {
                skill,
                content_changed,
            })
        }
        Err(e) => {
            let _ = store.update_skill_check_state(
                skill_id,
                Some(&remote_revision),
                "error",
                Some(&e.message),
            );
            Err(e)
        }
    }
}

pub fn reimport_local_skill_internal(
    store: &SkillStore,
    skill_id: &str,
) -> Result<ManagedSkillDto, AppError> {
    let skill = store
        .get_skill_by_id(skill_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Skill not found"))?;

    if !matches!(skill.source_type.as_str(), "local" | "import") {
        return Err(AppError::invalid_input(
            "Only local skills can be reimported",
        ));
    }

    let source_path = skill
        .source_ref
        .clone()
        .ok_or_else(|| AppError::not_found("Local skill is missing its original source path"))?;
    let path = PathBuf::from(&source_path);
    if !path.exists() {
        store
            .update_skill_check_state(
                &skill.id,
                None,
                "source_missing",
                Some("Original source path no longer exists"),
            )
            .map_err(AppError::db)?;
        return Err(AppError::not_found("Original source path no longer exists"));
    }

    store
        .update_skill_update_status(skill_id, "updating")
        .map_err(AppError::db)?;

    let result = (|| -> Result<(), AppError> {
        let _lock = RepoLock::acquire_foreground("reimport local skill")
            .map_err(AppError::db)?;
        let staged_path = staged_path_for(&skill.central_path);
        let install_result =
            installer::install_from_local_to_destination(&path, Some(&skill.name), &staged_path)
                .map_err(AppError::io)?;
        swap_skill_directory(&staged_path, Path::new(&skill.central_path))?;
        store
            .update_skill_after_install(
                &skill.id,
                &skill.name,
                install_result.description.as_deref(),
                None,
                None,
                Some(&install_result.content_hash),
                "local_only",
            )
            .map_err(AppError::db)?;
        resync_copy_targets(store, &skill.id)?;
        sync_metadata::write_all_from_db_unlocked(store).map_err(AppError::db)?;
        Ok(())
    })();

    match result {
        Ok(()) => managed_skill_by_id(store, skill_id),
        Err(e) => {
            let _ = store.update_skill_check_state(skill_id, None, "error", Some(&e.message));
            Err(e)
        }
    }
}

pub fn store_installed_skill_unlocked(
    store: &SkillStore,
    result: &installer::InstallResult,
    metadata: &InstallSourceMetadata,
    active_scenario_id: Option<&str>,
) -> Result<String, AppError> {
    let now = chrono::Utc::now().timestamp_millis();
    let central_path = result.central_path.to_string_lossy().to_string();

    if let Some(existing) = store
        .get_skill_by_central_path(&central_path)
        .map_err(AppError::db)?
    {
        store
            .update_skill_after_reinstall(
                &existing.id,
                &result.name,
                result.description.as_deref(),
                &metadata.source_type,
                metadata.source_ref.as_deref(),
                metadata.source_ref_resolved.as_deref(),
                metadata.source_subpath.as_deref(),
                metadata.source_branch.as_deref(),
                metadata.source_revision.as_deref(),
                metadata.remote_revision.as_deref(),
                Some(&result.content_hash),
                &metadata.update_status,
            )
            .map_err(AppError::db)?;
        if let Some(scenario_id) = active_scenario_id {
            store
                .add_skill_to_scenario(scenario_id, &existing.id)
                .map_err(AppError::db)?;
        }
        sync_metadata::write_all_from_db_unlocked(store).map_err(AppError::db)?;

        if let Some(scenario_id) = active_scenario_id {
            if let Err(e) =
                super::presets::sync_skill_to_active_preset(store, scenario_id, &existing.id)
            {
                log::warn!("Failed to sync reinstalled skill to preset: {e}");
            }
        }

        return Ok(existing.id);
    }

    let id = uuid::Uuid::new_v4().to_string();

    let record = SkillRecord {
        id: id.clone(),
        name: result.name.clone(),
        description: result.description.clone(),
        source_type: metadata.source_type.clone(),
        source_ref: metadata.source_ref.clone(),
        source_ref_resolved: metadata.source_ref_resolved.clone(),
        source_subpath: metadata.source_subpath.clone(),
        source_branch: metadata.source_branch.clone(),
        source_revision: metadata.source_revision.clone(),
        remote_revision: metadata.remote_revision.clone(),
        central_path,
        content_hash: Some(result.content_hash.clone()),
        enabled: true,
        created_at: now,
        updated_at: now,
        status: "ok".to_string(),
        update_status: metadata.update_status.clone(),
        last_checked_at: Some(now),
        last_check_error: None,
    };

    store.insert_skill(&record).map_err(AppError::db)?;
    if let Some(scenario_id) = active_scenario_id {
        store
            .add_skill_to_scenario(scenario_id, &id)
            .map_err(AppError::db)?;
    }
    sync_metadata::write_all_from_db_unlocked(store).map_err(AppError::db)?;

    if let Some(scenario_id) = active_scenario_id {
        if let Err(e) = super::presets::sync_skill_to_active_preset(store, scenario_id, &id) {
            log::warn!("Failed to sync newly installed skill to preset: {e}");
        }
    }

    Ok(id)
}

pub fn check_skill_update_internal(
    store: &SkillStore,
    skill_id: &str,
    force: bool,
    proxy_url: Option<&str>,
) -> Result<ManagedSkillDto, AppError> {
    let skill = store
        .get_skill_by_id(skill_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Skill not found"))?;

    if should_skip_update_check(store, &skill, force)? {
        return managed_skill_by_id(store, skill_id);
    }

    match skill.source_type.as_str() {
        "git" | "skillssh" => {
            let git_source = git_source_from_skill(&skill)?;
            let metadata_updated = skill.source_ref_resolved.as_deref()
                != Some(git_source.clone_url.as_str())
                || skill.source_subpath.as_deref() != git_source.subpath.as_deref()
                || skill.source_branch.as_deref() != git_source.branch.as_deref();
            if metadata_updated {
                store
                    .update_skill_source_metadata(
                        &skill.id,
                        Some(&git_source.clone_url),
                        git_source.subpath.as_deref(),
                        git_source.branch.as_deref(),
                        skill.source_revision.as_deref(),
                    )
                    .map_err(AppError::db)?;
            }

            match git_fetcher::resolve_remote_revision(
                &git_source.clone_url,
                git_source.branch.as_deref(),
                proxy_url,
            ) {
                Ok(remote_revision) => {
                    let update_status = match skill.source_revision.as_deref() {
                        Some(current) if current == remote_revision => "up_to_date",
                        Some(_) => "update_available",
                        None => "unknown",
                    };
                    store
                        .update_skill_check_state(
                            &skill.id,
                            Some(&remote_revision),
                            update_status,
                            None,
                        )
                        .map_err(AppError::db)?;
                }
                Err(err) => {
                    let message = err.to_string();
                    store
                        .update_skill_check_state(
                            &skill.id,
                            skill.remote_revision.as_deref(),
                            "error",
                            Some(&message),
                        )
                        .map_err(AppError::db)?;
                    return Err(AppError::git(message));
                }
            }
        }
        "local" | "import" => {
            let (status, error): (&str, Option<String>) = match skill.source_ref.as_deref() {
                Some(path) => {
                    let source_path = Path::new(path);
                    if !source_path.exists() {
                        (
                            "source_missing",
                            Some("Original source path no longer exists".to_string()),
                        )
                    } else {
                        match installer::hash_local_source(source_path) {
                            Ok(live_hash) => match skill.content_hash.as_deref() {
                                Some(stored) if stored == live_hash.as_str() => {
                                    ("up_to_date", None)
                                }
                                Some(_) => ("update_available", None),
                                None => ("local_only", None),
                            },
                            Err(err) => ("error", Some(err.to_string())),
                        }
                    }
                }
                None => ("local_only", None),
            };
            store
                .update_skill_check_state(&skill.id, None, status, error.as_deref())
                .map_err(AppError::db)?;
        }
        _ => {
            store
                .update_skill_check_state(&skill.id, None, "unknown", None)
                .map_err(AppError::db)?;
        }
    }

    managed_skill_by_id(store, skill_id)
}

fn should_skip_update_check(
    store: &SkillStore,
    skill: &SkillRecord,
    force: bool,
) -> Result<bool, AppError> {
    if force {
        return Ok(false);
    }

    let ttl_minutes = store
        .get_setting("update_check_ttl_minutes")
        .map_err(AppError::db)?
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(60);
    let ttl_ms = ttl_minutes * 60 * 1000;
    let stable_status = !matches!(
        skill.update_status.as_str(),
        "unknown" | "checking" | "updating" | "error"
    );

    Ok(stable_status
        && skill
            .last_checked_at
            .map(|checked| chrono::Utc::now().timestamp_millis() - checked < ttl_ms)
            .unwrap_or(false))
}

pub fn git_source_from_skill(skill: &SkillRecord) -> Result<GitSkillSource, AppError> {
    if let Some(resolved) = &skill.source_ref_resolved {
        return Ok(GitSkillSource {
            clone_url: resolved.clone(),
            branch: skill.source_branch.clone(),
            subpath: skill.source_subpath.clone(),
            locator_skill_id: skill_ssh_id(skill),
        });
    }

    match skill.source_type.as_str() {
        "git" => {
            let source_ref = skill
                .source_ref
                .as_ref()
                .ok_or_else(|| AppError::invalid_input("Git skill is missing its source URL"))?;
            let parsed = git_fetcher::parse_git_source(source_ref);
            Ok(GitSkillSource {
                clone_url: parsed.clone_url,
                // Prefer the branch resolved at install time — it survives
                // slash-branch tree URLs that the sync parse can't disambiguate.
                branch: skill.source_branch.clone().or(parsed.branch),
                subpath: skill.source_subpath.clone().or(parsed.subpath),
                locator_skill_id: None,
            })
        }
        "skillssh" => {
            let source_ref = skill.source_ref.as_ref().ok_or_else(|| {
                AppError::invalid_input("skills.sh skill is missing its source reference")
            })?;
            let (repo_source, fallback_skill_id) = source_ref
                .rsplit_once('/')
                .ok_or_else(|| AppError::invalid_input("Invalid skills.sh source reference"))?;
            Ok(GitSkillSource {
                clone_url: format!("https://github.com/{}.git", repo_source),
                branch: skill.source_branch.clone(),
                subpath: skill.source_subpath.clone(),
                locator_skill_id: Some(fallback_skill_id.to_string()),
            })
        }
        _ => Err(AppError::invalid_input(
            "Skill does not support git-based updates",
        )),
    }
}

fn skill_ssh_id(skill: &SkillRecord) -> Option<String> {
    if skill.source_type != "skillssh" {
        return None;
    }

    skill.source_ref.as_deref().and_then(|source_ref| {
        source_ref
            .rsplit_once('/')
            .map(|(_, skill_id)| skill_id.to_string())
    })
}

/// Return the list of individual skill directories to install from a resolved repo dir.
/// If `skill_dir` is itself a valid skill, returns `[skill_dir]`.
/// Otherwise recursively walks for skill dirs (e.g. `category/<skill>` layouts).
/// Returns an empty Vec when nothing is found — callers must handle that.
pub fn collect_git_skill_dirs(skill_dir: &Path) -> Vec<PathBuf> {
    if is_valid_skill_dir(skill_dir) {
        return vec![skill_dir.to_path_buf()];
    }
    let mut dirs = scanner::collect_skill_dirs(skill_dir);
    dirs.sort();
    dirs
}

/// Stable identifier for a discovered skill within a preview/confirm cycle.
/// Uses forward slashes regardless of platform so the frontend sees consistent keys.
pub fn skill_rel_key(skill_dir: &Path, dir: &Path) -> String {
    let rel = dir.strip_prefix(skill_dir).unwrap_or(dir);
    if rel.as_os_str().is_empty() {
        dir.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    } else {
        rel.to_string_lossy().replace('\\', "/")
    }
}

/// Validate and canonicalize a temp directory path used by the git preview/install flow.
/// Returns the canonicalized path if it passes security checks.
pub fn validate_clone_temp_path(temp_dir: &str) -> Result<PathBuf, AppError> {
    let raw_path = PathBuf::from(temp_dir);
    if !raw_path.exists() {
        return Err(AppError::invalid_input(
            "Clone session expired, please try again",
        ));
    }
    // Canonicalize to resolve symlinks and `..` segments before checking prefix.
    let temp_path = raw_path
        .canonicalize()
        .map_err(|_| AppError::invalid_input("Invalid temp directory"))?;

    // Preview confirmation must operate on an isolated checkout, never the repo cache.
    let expected_prefix = std::env::temp_dir()
        .canonicalize()
        .unwrap_or_else(|_| std::env::temp_dir());
    if temp_path.starts_with(&expected_prefix) {
        let dir_name_str = temp_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if dir_name_str.starts_with(git_fetcher::CLONE_TEMP_PREFIX) {
            return Ok(temp_path);
        }
    }

    Err(AppError::invalid_input("Invalid temp directory"))
}

pub fn resolve_skill_dir(
    repo_dir: &Path,
    subpath: Option<&str>,
    skill_id: Option<&str>,
) -> Result<PathBuf, AppError> {
    if let Some(subpath) = subpath {
        let path = repo_dir.join(subpath);
        if path.exists() && path.is_dir() {
            return Ok(path);
        }
    }

    git_fetcher::find_skill_dir(repo_dir, skill_id).map_err(AppError::git)
}

pub fn resolve_skillssh_install_target(
    store: &SkillStore,
    source_ref: &str,
    skill_id: &str,
    requested_name: Option<&str>,
) -> Result<(String, PathBuf), AppError> {
    if let Some(existing) = store
        .get_skill_by_source_ref("skillssh", source_ref)
        .map_err(AppError::db)?
    {
        return Ok((existing.name, PathBuf::from(existing.central_path)));
    }

    let base_name = requested_name.unwrap_or(skill_id).trim();
    let sanitized_name = skill_metadata::sanitize_skill_name(base_name)
        .ok_or_else(|| AppError::invalid_input("Skill name is empty or invalid"))?;
    if sanitized_name.is_empty() {
        return Err(AppError::invalid_input("Skill id is empty"));
    }

    let mut attempt = 1;
    loop {
        let candidate_name = if attempt == 1 {
            sanitized_name.clone()
        } else {
            format!("{sanitized_name}-{attempt}")
        };
        let candidate_path = central_repo::skills_dir().join(&candidate_name);
        let candidate_path_str = candidate_path.to_string_lossy().to_string();
        let occupied = store
            .get_skill_by_central_path(&candidate_path_str)
            .map_err(AppError::db)?
            .is_some();

        if !occupied {
            return Ok((candidate_name, candidate_path));
        }

        attempt += 1;
    }
}

pub fn staged_path_for(central_path: &str) -> PathBuf {
    let path = PathBuf::from(central_path);
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "skill".to_string());
    path.with_file_name(format!(".{file_name}.staged-{}", uuid::Uuid::new_v4()))
}

pub fn swap_skill_directory(staged_path: &Path, current_path: &Path) -> Result<(), AppError> {
    let backup_path = current_path.with_file_name(format!(
        ".{}.backup-{}",
        current_path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "skill".to_string()),
        uuid::Uuid::new_v4()
    ));

    if current_path.exists() {
        std::fs::rename(current_path, &backup_path)?;
    }

    if let Err(err) = std::fs::rename(staged_path, current_path) {
        if backup_path.exists() {
            let _ = std::fs::rename(&backup_path, current_path);
        }
        let _ = remove_path_if_exists(staged_path);
        return Err(err.into());
    }

    remove_path_if_exists(&backup_path)?;
    Ok(())
}

pub fn resync_copy_targets(store: &SkillStore, skill_id: &str) -> Result<(), AppError> {
    let skill = store
        .get_skill_by_id(skill_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Skill not found"))?;
    let source = PathBuf::from(&skill.central_path);
    let targets = store
        .get_targets_for_skill(skill_id)
        .map_err(AppError::db)?;

    for target in targets {
        if target.mode != "copy" {
            continue;
        }

        sync_engine::sync_skill(
            &source,
            Path::new(&target.target_path),
            sync_engine::SyncMode::Copy,
        )
        .map_err(AppError::io)?;

        let updated_target = SkillTargetRecord {
            synced_at: Some(chrono::Utc::now().timestamp_millis()),
            status: "ok".to_string(),
            last_error: None,
            // Refresh the hash so the startup freshness check (#153)
            // sees this resync as up-to-date instead of stale.
            source_hash: skill.content_hash.clone(),
            ..target
        };
        store.insert_target(&updated_target).map_err(AppError::db)?;
    }

    Ok(())
}

#[tauri::command]
pub async fn get_all_tags(store: State<'_, Arc<SkillStore>>) -> Result<Vec<String>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.get_all_tags().map_err(AppError::db)).await?
}

#[tauri::command]
pub async fn set_skill_tags(
    skill_id: String,
    tags: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        sync_metadata::with_repo_lock("set skill tags", || {
            store.set_tags_for_skill(&skill_id, &tags)?;
            sync_metadata::ensure_skill_metadata_unlocked(&store, &skill_id)
        })
        .map_err(AppError::db)
    })
    .await?
}

/// Globally rename a tag across all skills (used by the tag filter bar). If the
/// new name already exists, the tags are merged.
#[tauri::command]
pub async fn rename_tag(
    old_name: String,
    new_name: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let new_name = new_name.trim().to_string();
        if new_name.is_empty() {
            return Err(AppError::invalid_input("Tag name cannot be empty"));
        }
        if new_name == old_name {
            return Ok(());
        }
        sync_metadata::with_repo_lock("rename tag", || {
            let affected = store.rename_tag(&old_name, &new_name)?;
            for skill_id in &affected {
                sync_metadata::ensure_skill_metadata_unlocked(&store, skill_id)?;
            }
            Ok(())
        })
        .map_err(AppError::db)
    })
    .await?
}

/// Globally delete a tag from all skills (used by the tag filter bar).
#[tauri::command]
pub async fn delete_tag(name: String, store: State<'_, Arc<SkillStore>>) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        sync_metadata::with_repo_lock("delete tag", || {
            let affected = store.delete_tag(&name)?;
            for skill_id in &affected {
                sync_metadata::ensure_skill_metadata_unlocked(&store, skill_id)?;
            }
            Ok(())
        })
        .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn cancel_install(
    key: String,
    cancel_registry: State<'_, Arc<InstallCancelRegistry>>,
) -> Result<bool, AppError> {
    Ok(cancel_registry.cancel(&key))
}

#[derive(Debug, Serialize)]
pub struct BatchImportResult {
    pub imported: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

#[tauri::command]
pub async fn batch_import_folder(
    folder_path: String,
    store: State<'_, Arc<SkillStore>>,
    app_handle: tauri::AppHandle,
) -> Result<BatchImportResult, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Emitter;

        let root = PathBuf::from(&folder_path);
        if !root.is_dir() {
            return Err(AppError::invalid_input("Selected path is not a directory"));
        }

        // Collect valid skill subdirectories (depth=1)
        let mut skill_dirs: Vec<PathBuf> = Vec::new();
        let entries = std::fs::read_dir(&root)?;
        for entry in entries.flatten() {
            let path = entry.path();
            if is_valid_skill_dir(&path) {
                skill_dirs.push(path);
            }
        }

        if skill_dirs.is_empty() {
            return Ok(BatchImportResult {
                imported: 0,
                skipped: 0,
                errors: vec![],
            });
        }

        let total = skill_dirs.len();
        let mut imported = 0usize;
        let mut skipped = 0usize;
        let mut errors = Vec::new();

        for (i, dir) in skill_dirs.iter().enumerate() {
            let name = skill_metadata::infer_skill_name(dir);

            app_handle
                .emit(
                    "batch-import-progress",
                    serde_json::json!({
                        "current": i + 1,
                        "total": total,
                        "name": &name,
                    }),
                )
                .ok();

            // Check if already imported by prospective central path
            let prospective_central = central_repo::skills_dir().join(&name);
            let central_str = prospective_central.to_string_lossy().to_string();
            if let Ok(Some(_)) = store.get_skill_by_central_path(&central_str) {
                skipped += 1;
                continue;
            }

            let install_result = (|| -> Result<String, AppError> {
                let _lock = RepoLock::acquire_foreground("batch import skill")
                    .map_err(AppError::db)?;
                let result =
                    installer::install_from_local(dir, Some(&name)).map_err(AppError::io)?;
                let metadata = InstallSourceMetadata {
                    source_type: "local".to_string(),
                    source_ref: Some(dir.to_string_lossy().to_string()),
                    source_ref_resolved: None,
                    source_subpath: None,
                    source_branch: None,
                    source_revision: None,
                    remote_revision: None,
                    update_status: "local_only".to_string(),
                };
                store_installed_skill_unlocked(&store, &result, &metadata, None)
            })();

            match install_result {
                Ok(_) => imported += 1,
                Err(e) => errors.push(format!("{}: {}", name, e)),
            }
        }

        Ok(BatchImportResult {
            imported,
            skipped,
            errors,
        })
    })
    .await?
}

fn remove_path_if_exists(path: &Path) -> Result<(), AppError> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)?;
    } else if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::{tempdir, TempDir};

    struct TestRepo {
        _lock: std::sync::MutexGuard<'static, ()>,
        _tmp: TempDir,
        store: SkillStore,
    }

    impl Drop for TestRepo {
        fn drop(&mut self) {
            central_repo::set_test_base_dir_override(None);
        }
    }

    fn test_repo() -> TestRepo {
        let lock = central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        let base = tmp.path().join("repo");
        central_repo::set_test_base_dir_override(Some(base.clone()));
        fs::create_dir_all(central_repo::skills_dir()).unwrap();
        let store = SkillStore::new(&base.join("test.db")).unwrap();
        TestRepo {
            _lock: lock,
            _tmp: tmp,
            store,
        }
    }

    fn write_skill_dir(name: &str) -> PathBuf {
        let dir = central_repo::skills_dir().join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), format!("---\nname: {name}\n---\n")).unwrap();
        dir
    }

    fn sample_skill(id: &str, name: &str, central_path: &Path) -> SkillRecord {
        SkillRecord {
            id: id.to_string(),
            name: name.to_string(),
            description: None,
            source_type: "import".to_string(),
            source_ref: Some(central_path.to_string_lossy().to_string()),
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: central_path.to_string_lossy().to_string(),
            content_hash: None,
            enabled: true,
            created_at: 1,
            updated_at: 1,
            status: "ok".to_string(),
            update_status: "local_only".to_string(),
            last_checked_at: None,
            last_check_error: None,
        }
    }

    fn insert_sample_skill(repo: &TestRepo, id: &str, name: &str, central_path: &Path) {
        repo.store
            .insert_skill(&sample_skill(id, name, central_path))
            .unwrap();
    }

    #[test]
    fn skill_file_read_rejects_traversal_and_absolute_paths() {
        let repo = test_repo();
        let skill_dir = write_skill_dir("guarded-skill");
        insert_sample_skill(&repo, "skill-guard", "guarded-skill", &skill_dir);
        fs::write(repo._tmp.path().join("outside.txt"), "outside").unwrap();

        assert!(read_text_file_for_skill("skill-guard", &skill_dir, "../outside.txt").is_err());
        assert!(read_text_file_for_skill(
            "skill-guard",
            &skill_dir,
            &skill_dir.join("SKILL.md").to_string_lossy(),
        )
        .is_err());
    }

    #[test]
    fn skill_file_read_accepts_common_text_formats_and_rejects_binary_or_large_files() {
        let _repo = test_repo();
        let skill_dir = write_skill_dir("text-skill");
        fs::write(skill_dir.join("config.yaml"), "name: demo\n").unwrap();
        fs::write(skill_dir.join("data.json"), "{\"ok\":true}\n").unwrap();
        fs::write(skill_dir.join("notes.txt"), "hello\n").unwrap();
        fs::write(skill_dir.join("binary.bin"), [0, 159, 146, 150]).unwrap();
        fs::write(
            skill_dir.join("large.txt"),
            vec![b'a'; MAX_EDITABLE_FILE_BYTES as usize + 1],
        )
        .unwrap();

        for rel in ["SKILL.md", "config.yaml", "data.json", "notes.txt"] {
            let content = read_text_file_for_skill("skill-text", &skill_dir, rel).unwrap();
            assert_eq!(content.relative_path, rel);
            assert!(!content.hash.is_empty());
        }
        assert!(read_text_file_for_skill("skill-text", &skill_dir, "binary.bin").is_err());
        assert!(read_text_file_for_skill("skill-text", &skill_dir, "large.txt").is_err());
    }

    #[test]
    fn save_skill_file_rejects_hash_conflict() {
        let repo = test_repo();
        let skill_dir = write_skill_dir("conflict-skill");
        insert_sample_skill(&repo, "skill-conflict", "conflict-skill", &skill_dir);

        let err = save_skill_file_unlocked(
            &repo.store,
            "skill-conflict",
            "SKILL.md",
            "# changed\n",
            "not-the-current-hash",
        )
        .unwrap_err();

        assert!(err.message.contains("Reload before saving"));
        let content = fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
        assert!(content.contains("conflict-skill"));
    }

    #[test]
    fn save_skill_file_updates_content_hash_metadata_and_audit() {
        let repo = test_repo();
        let skill_dir = write_skill_dir("editable-skill");
        insert_sample_skill(&repo, "skill-edit", "editable-skill", &skill_dir);
        let current = read_text_file_for_skill("skill-edit", &skill_dir, "SKILL.md").unwrap();

        let updated = save_skill_file_unlocked(
            &repo.store,
            "skill-edit",
            "SKILL.md",
            "---\nname: editable-skill\ndescription: Updated\n---\n",
            &current.hash,
        )
        .unwrap();

        assert_eq!(updated.update_status, "local_changes");
        assert!(updated.updated_at > 1);
        assert!(fs::read_to_string(skill_dir.join("SKILL.md"))
            .unwrap()
            .contains("Updated"));
        assert!(repo
            .store
            .get_skill_by_id("skill-edit")
            .unwrap()
            .unwrap()
            .content_hash
            .is_some());
        assert!(sync_metadata::metadata_dir()
            .join("skills/skill-edit.json")
            .exists());
        let audit = repo.store.list_audit(Some(5)).unwrap();
        assert!(audit.iter().any(|entry| {
            entry.action == "edit"
                && entry.skill_id.as_deref() == Some("skill-edit")
                && entry.detail.as_deref() == Some("SKILL.md")
        }));
    }

    #[test]
    fn quality_check_reports_missing_doc_description_dangerous_command_and_missing_reference() {
        let _repo = test_repo();
        let missing_doc_dir = central_repo::skills_dir().join("missing-doc");
        fs::create_dir_all(&missing_doc_dir).unwrap();
        let missing_doc_issues = check_quality_for_dir(&missing_doc_dir).unwrap();
        assert!(missing_doc_issues
            .iter()
            .any(|issue| issue.code == "missing_document"));

        let skill_dir = central_repo::skills_dir().join("risky-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: risky-skill\n---\nSee [missing](missing.md)\nrm -rf /tmp/demo\n",
        )
        .unwrap();
        let issues = check_quality_for_dir(&skill_dir).unwrap();
        assert!(issues.iter().any(|issue| issue.code == "missing_description"));
        assert!(issues.iter().any(|issue| issue.code == "dangerous_command"));
        assert!(issues.iter().any(|issue| issue.code == "missing_reference"));
    }

    #[test]
    fn skillssh_preview_parses_multifile_skill_fixture() {
        let _repo = test_repo();
        let skill_dir = central_repo::skills_dir().join("fixture-market-skill");
        fs::create_dir_all(skill_dir.join("references")).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: Fixture Skill\ndescription: Fixture description\n---\n# Fixture\n",
        )
        .unwrap();
        fs::write(skill_dir.join("references").join("guide.md"), "# Guide\n").unwrap();
        fs::write(skill_dir.join("config.json"), "{\"enabled\":true}\n").unwrap();

        let preview =
            build_market_skill_preview_from_dir("owner/repo", "fixture-market-skill", &skill_dir)
                .unwrap();
        let files = flatten_test_file_paths(&preview.files);

        assert_eq!(preview.source, "owner/repo");
        assert_eq!(preview.skill_id, "fixture-market-skill");
        assert_eq!(preview.name, "Fixture Skill");
        assert_eq!(preview.description.as_deref(), Some("Fixture description"));
        assert!(preview.document.unwrap().content.contains("# Fixture"));
        assert!(files.contains(&"SKILL.md".to_string()));
        assert!(files.contains(&"config.json".to_string()));
        assert!(files.contains(&"references/guide.md".to_string()));
        assert!(preview.risk_issues.is_empty());
    }

    fn flatten_test_file_paths(nodes: &[SkillFileNodeDto]) -> Vec<String> {
        let mut paths = Vec::new();
        for node in nodes {
            if node.kind == "file" {
                paths.push(node.relative_path.clone());
            }
            if let Some(children) = &node.children {
                paths.extend(flatten_test_file_paths(children));
            }
        }
        paths
    }

    #[test]
    fn batch_delete_removes_skills_targets_and_stale_metadata_once() {
        let repo = test_repo();
        let skill_one_dir = write_skill_dir("skill-one");
        let skill_two_dir = write_skill_dir("skill-two");
        repo.store
            .insert_skill(&sample_skill("skill-1", "skill-one", &skill_one_dir))
            .unwrap();
        repo.store
            .insert_skill(&sample_skill("skill-2", "skill-two", &skill_two_dir))
            .unwrap();

        let target_dir = repo._tmp.path().join("target-skill-one");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("SKILL.md"), "# target").unwrap();
        repo.store
            .insert_target(&SkillTargetRecord {
                id: "target-1".to_string(),
                skill_id: "skill-1".to_string(),
                tool: "cursor".to_string(),
                target_path: target_dir.to_string_lossy().to_string(),
                mode: "symlink".to_string(),
                status: "ok".to_string(),
                synced_at: Some(1),
                last_error: None,
                source_hash: None,
            })
            .unwrap();

        sync_metadata::write_all_from_db_unlocked(&repo.store).unwrap();
        assert!(sync_metadata::metadata_dir()
            .join("skills/skill-1.json")
            .exists());
        assert!(sync_metadata::metadata_dir()
            .join("skills/skill-2.json")
            .exists());

        let result = delete_managed_skills_by_ids(
            &repo.store,
            &["skill-1".to_string(), "missing-skill".to_string()],
        )
        .unwrap();

        assert_eq!(result.deleted, 1);
        assert_eq!(result.failed, vec!["missing-skill".to_string()]);
        assert!(repo.store.get_skill_by_id("skill-1").unwrap().is_none());
        assert!(repo.store.get_skill_by_id("skill-2").unwrap().is_some());
        assert!(!skill_one_dir.exists());
        assert!(skill_two_dir.exists());
        assert!(!target_dir.exists());
        assert!(!sync_metadata::metadata_dir()
            .join("skills/skill-1.json")
            .exists());
        assert!(sync_metadata::metadata_dir()
            .join("skills/skill-2.json")
            .exists());
    }

    fn write_skill_at(root: &Path, rel: &str) -> PathBuf {
        let dir = root.join(rel);
        fs::create_dir_all(&dir).unwrap();
        let basename = dir.file_name().unwrap().to_string_lossy().to_string();
        fs::write(dir.join("SKILL.md"), format!("---\nname: {basename}\n---\n")).unwrap();
        dir
    }

    #[test]
    fn collect_git_skill_dirs_finds_nested_categories() {
        // Mirrors mattpocock/skills layout: skills/<category>/<skill>/SKILL.md.
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        write_skill_at(root, "in-progress/foo");
        write_skill_at(root, "in-progress/bar");
        write_skill_at(root, "stable/baz");

        let dirs = collect_git_skill_dirs(root);
        let keys: Vec<String> = dirs.iter().map(|d| skill_rel_key(root, d)).collect();
        assert_eq!(dirs.len(), 3, "should find skills two levels deep");
        assert!(keys.contains(&"in-progress/foo".to_string()));
        assert!(keys.contains(&"in-progress/bar".to_string()));
        assert!(keys.contains(&"stable/baz".to_string()));
    }

    #[test]
    fn collect_git_skill_dirs_returns_self_when_root_is_skill() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join("SKILL.md"), "---\nname: x\n---").unwrap();
        let dirs = collect_git_skill_dirs(root);
        assert_eq!(dirs, vec![root.to_path_buf()]);
    }

    #[test]
    fn collect_git_skill_dirs_returns_empty_when_no_skills() {
        // Previously this case returned [skill_dir] as a bogus fallback,
        // which then surfaced a non-skill category dir as installable.
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("empty-category")).unwrap();
        let dirs = collect_git_skill_dirs(root);
        assert!(dirs.is_empty(), "no fallback to scan root when empty");
    }

    #[test]
    fn skill_rel_key_uses_forward_slashes() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("repo");
        let nested = root.join("a").join("b");
        let key = skill_rel_key(&root, &nested);
        assert_eq!(key, "a/b");
    }

    #[test]
    fn skill_rel_key_disambiguates_same_basename_across_categories() {
        // Two skills with the same dir basename in different categories must
        // produce distinct rel keys — that's the point of using rel paths.
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        let a_foo = write_skill_at(root, "category-a/foo");
        let b_foo = write_skill_at(root, "category-b/foo");

        let dirs = collect_git_skill_dirs(root);
        assert_eq!(dirs.len(), 2);

        let k_a = skill_rel_key(root, &a_foo);
        let k_b = skill_rel_key(root, &b_foo);
        assert_ne!(k_a, k_b);
        assert_eq!(k_a, "category-a/foo");
        assert_eq!(k_b, "category-b/foo");
    }
}
