use anyhow::Result;

use super::context::DeleteContext;
use super::stages::DeleteStage;

/// Execute a single delete stage.
pub fn execute_stage(stage: &DeleteStage, ctx: &DeleteContext) -> Result<()> {
    match stage {
        DeleteStage::Stop => stage_stop(ctx),
        DeleteStage::Release => stage_release(ctx),
        DeleteStage::Cleanup => stage_cleanup(ctx),
        DeleteStage::Remove => stage_remove(ctx),
        DeleteStage::Finalize => stage_finalize(ctx),
    }
}

// ── Stage 1: Stop ──

fn stage_stop(ctx: &DeleteContext) -> Result<()> {
    for item in &ctx.cleanup_items {
        // Stop is done by caller before building context (TUI needs immediate stop for UI)
        // This stage ensures any remaining services are stopped
        let _ = item; // cleanup_items carry the info, actual stop handled by caller
    }
    Ok(())
}

// ── Stage 2: Release ──

fn stage_release(ctx: &DeleteContext) -> Result<()> {
    let ws_key = format!("ws-{}", ctx.branch);
    for (svc_name, _) in &ctx.config.shared_services {
        crate::services::release_slot(svc_name, &ws_key);
    }
    crate::services::release_ip(&ws_key);
    Ok(())
}

// ── Stage 3: Cleanup ──

fn stage_cleanup(ctx: &DeleteContext) -> Result<()> {
    for item in &ctx.cleanup_items {
        if !item.pre_delete.is_empty() && item.wt_path.exists() {
            let combined = item.pre_delete.join(" && ");
            let _ = std::process::Command::new("zsh")
                .args(["-c", &combined])
                .current_dir(&item.wt_path)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
    Ok(())
}

// ── Stage 4: Remove ──

fn stage_remove(ctx: &DeleteContext) -> Result<()> {
    // Remove git worktrees
    for item in &ctx.cleanup_items {
        let _ = crate::services::remove_worktree(
            std::path::Path::new(&item.dir_path), &item.wt_path, &item.wt_branch,
        );
    }

    // Drop databases (batch — single container)
    if !ctx.dbs_to_drop.is_empty() {
        let db_names: Vec<String> = ctx.dbs_to_drop.iter().map(|db| db.db_name.clone()).collect();
        let first = &ctx.dbs_to_drop[0];
        crate::services::drop_shared_dbs_batch(&first.host, first.port, &db_names, &first.user, &first.password);
    }

    Ok(())
}

// ── Stage 5: Finalize ──

fn stage_finalize(ctx: &DeleteContext) -> Result<()> {
    crate::services::remove_docker_network(&ctx.network);
    crate::services::delete_workspace_folder(&ctx.config_dir, &ctx.branch);
    Ok(())
}
