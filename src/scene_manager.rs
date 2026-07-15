use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::world::World;

/// Saved scene data.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SceneState {
    pub name: String,
    pub width: usize,
    pub height: usize,
    pub grid: Vec<u8>,
    pub life: Vec<u16>,
    pub seed: Vec<u8>,
    /// Optional for backward compatibility with older scene files.
    #[serde(default)]
    pub vx: Vec<i8>,
    /// Optional for backward compatibility with older scene files.
    #[serde(default)]
    pub vy: Vec<i8>,
    /// Fractional vertical velocity in quarter-cell units.
    #[serde(default)]
    pub vy_frac: Vec<i8>,
    /// Fractional vertical displacement in quarter-cell units.
    #[serde(default)]
    pub y_frac: Vec<i8>,
    /// Optional for backward compatibility with older scene files.
    #[serde(default)]
    pub temp: Vec<i16>,
    pub saved_at: u64,
}

impl SceneState {
    /// Snapshot the world into a serializable state.
    pub fn from_world(world: &World, name: String) -> Self {
        let grid: Vec<u8> = world.grid().iter().map(|m| m.to_u8()).collect();
        let life = world.life().to_vec();
        let seed = world.seed().to_vec();
        let vx = world.vx().to_vec();
        let vy = world.vy().to_vec();
        let vy_frac = world.vy_frac().to_vec();
        let y_frac = world.y_frac().to_vec();
        let temp = world.temp().to_vec();
        Self {
            name,
            width: world.width,
            height: world.height,
            grid,
            life,
            seed,
            vx,
            vy,
            vy_frac,
            y_frac,
            temp,
            saved_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        }
    }
}

// ---- Test isolation ----
#[cfg(test)]
static TEST_SCENE_DIR_OVERRIDE: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);
/// Serialises tests that need the scene-dir override so they don't race.
#[cfg(test)]
static TEST_SERIAL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Directory where scenes are stored.
fn scene_dir() -> PathBuf {
    #[cfg(test)]
    {
        let guard = TEST_SCENE_DIR_OVERRIDE.lock().unwrap();
        if let Some(ref dir) = *guard {
            return dir.clone();
        }
    }
    let mut dir = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("."));
    dir.pop(); // binary dir
    dir.push("scenes");
    dir
}

/// Validate that `name` is a safe scene file name.
///
/// Rejects:
/// - Empty or whitespace-only names.
/// - Names containing path separators (`/` or `\`) or the parent‑dir component `..`.
/// - Names that are just `.`.
/// - Names that start with `.` (to avoid hidden files on Unix).
/// - Names longer than 255 bytes.
fn validate_scene_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.trim().is_empty() {
        return Err("Scene name cannot be empty or whitespace only".into());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("Scene name must not contain path separators".into());
    }
    if name == "." || name == ".." || name.contains("..") {
        return Err("Scene name must not contain '.' or '..'".into());
    }
    if name.starts_with('.') {
        return Err("Scene name must not start with '.'".into());
    }
    if name.len() > 255 {
        return Err("Scene name must not exceed 255 bytes".into());
    }
    Ok(())
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
        if path.extension().is_some_and(|e| e == "json")
            && let Some(stem) = path.file_stem()
        {
            names.push(stem.to_string_lossy().to_string());
        }
    }
    names.sort();
    Ok(names)
}

/// Load a scene's state by name.
pub fn load_scene_state(name: &str) -> Result<SceneState, String> {
    validate_scene_name(name)?;
    let path = scene_dir().join(format!("{name}.json"));
    let data = fs::read_to_string(&path).map_err(|e| format!("Cannot read {name}: {e}"))?;
    serde_json::from_str(&data).map_err(|e| format!("Invalid scene data: {e}"))
}

/// Save a scene state to disk.
pub fn save_scene_state(state: &SceneState) -> Result<(), String> {
    validate_scene_name(&state.name)?;
    let dir = scene_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create scenes dir: {e}"))?;
    let path = dir.join(format!("{}.json", state.name));
    let data = serde_json::to_string_pretty(state).map_err(|e| format!("Serialize error: {e}"))?;
    fs::write(&path, data).map_err(|e| format!("Cannot write scene: {e}"))
}

/// Delete a scene by name.
pub fn delete_scene(name: &str) -> Result<(), String> {
    validate_scene_name(name)?;
    let path = scene_dir().join(format!("{name}.json"));
    fs::remove_file(&path).map_err(|e| format!("Cannot delete {name}: {e}"))?;
    Ok(())
}

// ---- Tests ----
#[cfg(test)]
mod tests {
    use super::*;

    /// Sets up a temporary test directory and overrides `scene_dir()`.
    /// The guard holds TEST_SERIAL_LOCK so only one TestDir-using test runs at
    /// a time, preventing races on the shared override.
    struct TestDir {
        _serial: std::sync::MutexGuard<'static, ()>,
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let _serial = TEST_SERIAL_LOCK.lock().unwrap();
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let path = std::env::temp_dir().join(format!("physics_sandbox_test_{unique}"));
            fs::create_dir_all(&path).expect("create test scene dir");
            *TEST_SCENE_DIR_OVERRIDE.lock().unwrap() = Some(path.clone());
            Self { _serial, path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            *TEST_SCENE_DIR_OVERRIDE.lock().unwrap() = None;
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn dummy_state(name: &str) -> SceneState {
        SceneState {
            name: name.into(),
            width: 4,
            height: 4,
            grid: vec![0; 16],
            life: vec![0; 16],
            seed: vec![0; 16],
            vx: vec![0; 16],
            vy: vec![0; 16],
            vy_frac: vec![0; 16],
            y_frac: vec![0; 16],
            temp: vec![20; 16],
            saved_at: 0,
        }
    }

    // ---- Name validation ----

    #[test]
    fn rejects_empty_name() {
        let err = validate_scene_name("").unwrap_err();
        assert!(err.contains("empty"), "got: {err}");
    }

    #[test]
    fn rejects_whitespace_only() {
        let err = validate_scene_name("   ").unwrap_err();
        assert!(
            err.contains("empty") || err.contains("whitespace"),
            "got: {err}"
        );
    }

    #[test]
    fn rejects_path_separator_slash() {
        let err = validate_scene_name("a/b").unwrap_err();
        assert!(err.contains("separator"), "got: {err}");
    }

    #[test]
    fn rejects_path_separator_backslash() {
        let err = validate_scene_name("a\\b").unwrap_err();
        assert!(err.contains("separator"), "got: {err}");
    }

    #[test]
    fn rejects_dot() {
        let err = validate_scene_name(".").unwrap_err();
        assert!(err.contains('\''), "got: {err}");
    }

    #[test]
    fn rejects_dot_dot() {
        let err = validate_scene_name("..").unwrap_err();
        assert!(err.contains('\''), "got: {err}");
    }

    #[test]
    fn rejects_leading_dot() {
        let err = validate_scene_name(".hidden").unwrap_err();
        assert!(err.contains('\''), "got: {err}");
    }

    #[test]
    fn accepts_valid_name() {
        validate_scene_name("my_scene").unwrap();
        validate_scene_name("House Cross-Section").unwrap();
        validate_scene_name("test_01").unwrap();
    }

    // ---- Round-trip save/load/delete ----

    #[test]
    fn round_trip_save_and_load() {
        let _td = TestDir::new();
        let mut state = dummy_state("roundtrip_test");
        state.vx[3] = -4;
        state.vy[3] = 2;
        state.vy_frac[3] = -3;
        state.y_frac[3] = 2;
        save_scene_state(&state).unwrap();

        let loaded = load_scene_state("roundtrip_test").unwrap();
        assert_eq!(loaded, state);
    }

    #[test]
    fn restore_clamps_velocity() {
        let mut state = dummy_state("clamped");
        state.vx[3] = i8::MIN;
        state.vy[3] = i8::MAX;
        state.vy_frac[3] = i8::MIN;
        state.y_frac[3] = i8::MAX;
        let mut world = World::new(4, 4);

        world.restore_from(&state);

        assert_eq!(world.velocity_at(3, 0), (-4, 4));
        assert_eq!(world.vy_frac()[3], -3);
        assert_eq!(world.y_frac()[3], 3);
    }

    #[test]
    fn old_scene_without_velocity_defaults_to_zero() {
        let state: SceneState = serde_json::from_str(
            r#"{
                "name":"old",
                "width":2,
                "height":1,
                "grid":[0,1],
                "life":[0,0],
                "seed":[1,2],
                "temp":[20,20],
                "saved_at":0
            }"#,
        )
        .unwrap();

        assert!(state.vx.is_empty());
        assert!(state.vy.is_empty());
        assert!(state.vy_frac.is_empty());
        assert!(state.y_frac.is_empty());
        let mut world = World::new(2, 1);
        world.restore_from(&state);
        assert_eq!(world.velocity_at(0, 0), (0, 0));
        assert_eq!(world.velocity_at(1, 0), (0, 0));
    }

    #[test]
    fn list_includes_saved_scene() {
        let _td = TestDir::new();
        save_scene_state(&dummy_state("listme")).unwrap();
        let names = list_scenes().unwrap();
        assert!(names.contains(&"listme".into()), "names: {names:?}");
    }

    #[test]
    fn delete_removes_scene() {
        let _td = TestDir::new();
        save_scene_state(&dummy_state("deleteme")).unwrap();
        delete_scene("deleteme").unwrap();
        let err = load_scene_state("deleteme").unwrap_err();
        assert!(err.contains("Cannot read"), "got: {err}");
    }

    // ---- Malicious / escaped path rejections ----

    #[test]
    fn reject_traversal_in_load() {
        let _td = TestDir::new();
        let err = load_scene_state("../etc/passwd").unwrap_err();
        assert!(
            !err.contains("Cannot read"),
            "should be rejected before I/O: {err}"
        );
    }

    #[test]
    fn reject_traversal_in_save() {
        let _td = TestDir::new();
        let state = dummy_state("../escape");
        let err = save_scene_state(&state).unwrap_err();
        assert!(
            !err.contains("Cannot write"),
            "should be rejected before I/O: {err}"
        );
    }

    #[test]
    fn reject_traversal_in_delete() {
        let _td = TestDir::new();
        let err = delete_scene("../escape").unwrap_err();
        assert!(
            !err.contains("Cannot delete"),
            "should be rejected before I/O: {err}"
        );
    }

    #[test]
    fn reject_backslash_traversal() {
        let _td = TestDir::new();
        let err = load_scene_state("..\\escape").unwrap_err();
        assert!(
            !err.contains("Cannot read"),
            "should be rejected before I/O: {err}"
        );
    }

    #[test]
    fn reject_contains_dot_dot() {
        let _td = TestDir::new();
        let err = load_scene_state("foo..bar").unwrap_err();
        assert!(!err.contains("Cannot read"), "should be rejected: {err}");
    }
}
