pub mod stages;
pub mod context;
pub mod create;
pub mod delete;

use std::path::PathBuf;
use std::sync::mpsc;

use stages::{CreateStage, DeleteStage};

// ── Pipeline Events ──

/// Events sent from pipeline thread to consumer (TUI or CLI).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum PipelineEvent {
    StageStarted { branch: String, index: usize, name: String, total: usize },
    StageCompleted { branch: String, index: usize },
    StageSkipped { branch: String, index: usize },
    PipelineCompleted { branch: String },
    PipelineFailed { branch: String, stage: usize, error: String },
}

// ── Pipeline State (for persistence + retry) ──
#[allow(dead_code)]

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum StageStatus {
    Pending,
    Completed,
    Skipped,
    Failed(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StageEntry {
    pub name: String,
    pub status: StageStatus,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum PipelineOp {
    CreateWorkspace,
    DeleteWorkspace,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PipelineState {
    pub operation: PipelineOp,
    pub branch: String,
    pub workspace: String,
    pub stages: Vec<StageEntry>,
    pub failed_stage: usize,
}

// ── Active Pipeline Markers ──

fn active_marker_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".tncli/active")
}

pub fn mark_pipeline_active(branch: &str, stage: usize, total: usize, stage_name: &str) {
    let dir = active_marker_dir();
    let _ = std::fs::create_dir_all(&dir);
    let content = format!("{stage}/{total} {stage_name}");
    let _ = std::fs::write(dir.join(branch.replace('/', "-")), content);
}

pub fn mark_pipeline_done(branch: &str) {
    let path = active_marker_dir().join(branch.replace('/', "-"));
    let _ = std::fs::remove_file(path);
}

/// Returns (branch_safe, current_stage, total_stages, stage_name)
pub fn list_active_pipelines() -> Vec<(String, usize, usize, String)> {
    let dir = active_marker_dir();
    std::fs::read_dir(&dir)
        .map(|entries| entries.filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                let content = std::fs::read_to_string(e.path()).unwrap_or_default();
                let parts: Vec<&str> = content.splitn(2, ' ').collect();
                let (current, total) = if let Some(ratio) = parts.first() {
                    let nums: Vec<&str> = ratio.split('/').collect();
                    (nums.first().and_then(|n| n.parse().ok()).unwrap_or(0),
                     nums.get(1).and_then(|n| n.parse().ok()).unwrap_or(7))
                } else { (0, 7) };
                let stage_name = parts.get(1).unwrap_or(&"").to_string();
                Some((name, current, total, stage_name))
            })
            .collect())
        .unwrap_or_default()
}

// ── State Persistence ──

fn pipeline_state_path(branch: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(format!(".tncli/pipeline-{}.json", branch.replace('/', "-")))
}

pub fn save_pipeline_state(branch: &str, workspace: &str, op: PipelineOp, stage_labels: &[&str], failed_stage: usize, error: &str) {
    let stages: Vec<StageEntry> = stage_labels.iter().enumerate().map(|(i, name)| {
        let status = if i < failed_stage {
            StageStatus::Completed
        } else if i == failed_stage {
            StageStatus::Failed(error.to_string())
        } else {
            StageStatus::Pending
        };
        StageEntry { name: name.to_string(), status }
    }).collect();

    let state = PipelineState {
        operation: op,
        branch: branch.to_string(),
        workspace: workspace.to_string(),
        stages,
        failed_stage,
    };

    let path = pipeline_state_path(branch);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(&state) {
        let _ = std::fs::write(&path, json);
    }
}

#[allow(dead_code)]
pub fn load_pipeline_state(branch: &str) -> Option<PipelineState> {
    let path = pipeline_state_path(branch);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

pub fn clear_pipeline_state(branch: &str) {
    let path = pipeline_state_path(branch);
    let _ = std::fs::remove_file(&path);
}

// ── Pipeline Runner ──

pub fn run_create_pipeline(ctx: context::CreateContext, tx: mpsc::Sender<PipelineEvent>) {
    let stages = CreateStage::all();
    let total = stages.len();
    let branch = ctx.branch.clone();
    let mut state = create::CreateState::new(&ctx);

    mark_pipeline_active(&branch, 0, total, stages[0].label());

    for (i, stage) in stages.iter().enumerate() {
        if ctx.skip_stages.contains(&i) {
            let _ = tx.send(PipelineEvent::StageSkipped { branch: branch.clone(), index: i });
            continue;
        }

        mark_pipeline_active(&branch, i, total, stage.label());
        let _ = tx.send(PipelineEvent::StageStarted {
            branch: branch.clone(),
            index: i,
            name: stage.label().to_string(),
            total,
        });

        match create::execute_stage(stage, &ctx, &mut state) {
            Ok(()) => {
                let _ = tx.send(PipelineEvent::StageCompleted { branch: branch.clone(), index: i });
            }
            Err(e) => {
                let labels: Vec<&str> = stages.iter().map(|s| s.label()).collect();
                save_pipeline_state(&ctx.branch, &ctx.workspace_name, PipelineOp::CreateWorkspace, &labels, i, &e.to_string());
                mark_pipeline_done(&branch);
                let _ = tx.send(PipelineEvent::PipelineFailed { branch: branch.clone(), stage: i, error: e.to_string() });
                return;
            }
        }
    }

    clear_pipeline_state(&ctx.branch);
    mark_pipeline_done(&branch);
    let _ = tx.send(PipelineEvent::PipelineCompleted { branch });
}

pub fn run_delete_pipeline(ctx: context::DeleteContext, tx: mpsc::Sender<PipelineEvent>) {
    let stages = DeleteStage::all();
    let total = stages.len();
    let branch = ctx.branch.clone();

    mark_pipeline_active(&branch, 0, total, stages[0].label());

    for (i, stage) in stages.iter().enumerate() {
        if ctx.skip_stages.contains(&i) {
            let _ = tx.send(PipelineEvent::StageSkipped { branch: branch.clone(), index: i });
            continue;
        }

        mark_pipeline_active(&branch, i, total, stage.label());
        let _ = tx.send(PipelineEvent::StageStarted {
            branch: branch.clone(),
            index: i,
            name: stage.label().to_string(),
            total,
        });

        match delete::execute_stage(stage, &ctx) {
            Ok(()) => {
                let _ = tx.send(PipelineEvent::StageCompleted { branch: branch.clone(), index: i });
            }
            Err(e) => {
                let labels: Vec<&str> = stages.iter().map(|s| s.label()).collect();
                save_pipeline_state(&ctx.branch, "", PipelineOp::DeleteWorkspace, &labels, i, &e.to_string());
                mark_pipeline_done(&branch);
                let _ = tx.send(PipelineEvent::PipelineFailed { branch: branch.clone(), stage: i, error: e.to_string() });
                return;
            }
        }
    }

    clear_pipeline_state(&ctx.branch);
    mark_pipeline_done(&branch);
    let _ = tx.send(PipelineEvent::PipelineCompleted { branch });
}
