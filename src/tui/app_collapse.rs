pub(crate) fn collapse_state_path(session: &str) -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    std::path::PathBuf::from(home).join(format!(".tncli/collapse-{session}.json"))
}

pub(crate) fn load_collapse_state(
    session: &str,
    _dir_names: &[String],
) -> (Vec<bool>, std::collections::HashMap<String, bool>, std::collections::HashMap<String, bool>) {
    let path = collapse_state_path(session);
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return (Vec::new(), Default::default(), Default::default()),
    };
    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return (Vec::new(), Default::default(), Default::default()),
    };

    let mut wt_collapsed: std::collections::HashMap<String, bool> = Default::default();
    if let Some(wt) = json.get("wt").and_then(|v| v.as_object()) {
        for (k, v) in wt {
            if let Some(b) = v.as_bool() { wt_collapsed.insert(k.clone(), b); }
        }
    }

    let mut combo_collapsed: std::collections::HashMap<String, bool> = Default::default();
    if let Some(cb) = json.get("combo").and_then(|v| v.as_object()) {
        for (k, v) in cb {
            if let Some(b) = v.as_bool() { combo_collapsed.insert(k.clone(), b); }
        }
    }

    (Vec::new(), wt_collapsed, combo_collapsed)
}

pub(crate) fn save_collapse_state(
    session: &str,
    _dir_names: &[String],
    wt_collapsed: &std::collections::HashMap<String, bool>,
    combo_collapsed: &std::collections::HashMap<String, bool>,
) {
    let wt: serde_json::Map<String, serde_json::Value> = wt_collapsed.iter()
        .filter(|(_, v)| **v)
        .map(|(k, v)| (k.clone(), serde_json::Value::Bool(*v)))
        .collect();
    let combo: serde_json::Map<String, serde_json::Value> = combo_collapsed.iter()
        .filter(|(_, v)| **v)
        .map(|(k, v)| (k.clone(), serde_json::Value::Bool(*v)))
        .collect();

    let json = serde_json::json!({ "wt": wt, "combo": combo });
    let path = collapse_state_path(session);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, serde_json::to_string_pretty(&json).unwrap_or_default());
}
