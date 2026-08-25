use crate::db::Database;
use crate::services::inventory::{discover_inventory, InventorySnapshot};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::State;

#[tauri::command]
pub async fn get_tool_inventory(
    db: State<'_, Arc<Mutex<Database>>>,
) -> Result<InventorySnapshot, String> {
    let project_roots = {
        let database = db
            .lock()
            .map_err(|_| "Could not access the project list.".to_string())?;
        database
            .get_all_projects()
            .map_err(|_| "Could not load the project list.".to_string())?
            .into_iter()
            .map(|project| PathBuf::from(project.path))
            .collect()
    };
    let home_dir =
        dirs::home_dir().ok_or_else(|| "Could not locate the home folder.".to_string())?;
    tokio::task::spawn_blocking(move || discover_inventory(home_dir, project_roots))
        .await
        .map_err(|_| "The inventory scan could not be completed.".to_string())
}
