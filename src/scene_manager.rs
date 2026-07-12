use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::world::World;

/// Saved scene data.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SceneState {
    pub name: String,
    pub width: usize,
    pub height: usize,
    pub grid: Vec<u8>,
    pub life: Vec<u16>,
    pub seed: Vec<u8>,
    pub saved_at: u64,
}

impl SceneState {
    /// Snapshot the world into a serializable state.
    pub fn from_world(world: &World, name: String) -> Self {
        let grid: Vec<u8> = world.grid().iter().map(|m| m.to_u8()).collect();
        let life = world.life().to_vec();
        let seed = world.seed().to_vec();
        Self {
            name,
            width: world.width,
            height: world.height,
            grid,
            life,
            seed,
            saved_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        }
    }
}

/// Directory where scenes are stored.
fn scene_dir() -> PathBuf {
    let mut dir = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("."));
    dir.pop(); // binary dir
    dir.push("scenes");
    dir
}

/// List all saved scene names (filenames without .json).
pub fn list_scenes() -> Result<Vec<String>, String> {
    let dir = scene_dir();
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "json") {
            if let Some(stem) = path.file_stem() {
                names.push(stem.to_string_lossy().to_string());
            }
        }
    }
    names.sort();
    Ok(names)
}

/// Load a scene's state by name.
pub fn load_scene_state(name: &str) -> Result<SceneState, String> {
    let path = scene_dir().join(format!("{name}.json"));
    let data = fs::read_to_string(&path).map_err(|e| format!("Cannot read {name}: {e}"))?;
    serde_json::from_str(&data).map_err(|e| format!("Invalid scene data: {e}"))
}

/// Save a scene state to disk.
pub fn save_scene_state(state: &SceneState) -> Result<(), String> {
    let dir = scene_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create scenes dir: {e}"))?;
    let path = dir.join(format!("{}.json", state.name));
    let data = serde_json::to_string_pretty(state).map_err(|e| format!("Serialize error: {e}"))?;
    fs::write(&path, data).map_err(|e| format!("Cannot write scene: {e}"))
}

/// Delete a scene by name.
pub fn delete_scene(name: &str) -> Result<(), String> {
    let path = scene_dir().join(format!("{name}.json"));
    fs::remove_file(&path).map_err(|e| format!("Cannot delete {name}: {e}"))?;
    Ok(())
}
