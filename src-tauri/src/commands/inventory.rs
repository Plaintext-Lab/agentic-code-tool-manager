use crate::db::Database;
use crate::services::inventory::{
    discover_inventory, set_inventory_record_enabled as set_inventory_record_enabled_service,
    InventoryActionRequest, InventorySnapshot,
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::State;

#[tauri::command]
pub async fn get_tool_inventory(
    db: State<'_, Arc<Mutex<Database>>>,
) -> Result<InventorySnapshot, String> {
    let project_roots = project_roots(&db)?;
    let home_dir =
        dirs::home_dir().ok_or_else(|| "Could not locate the home folder.".to_string())?;
    tokio::task::spawn_blocking(move || discover_inventory(home_dir, project_roots))
        .await
        .map_err(|_| "The inventory scan could not be completed.".to_string())
}

#[tauri::command]
pub async fn set_inventory_record_enabled(
    db: State<'_, Arc<Mutex<Database>>>,
    record_id: String,
    enabled: bool,
    source_revision: String,
) -> Result<InventorySnapshot, String> {
    let project_roots = project_roots(&db)?;
    let home_dir =
        dirs::home_dir().ok_or_else(|| "Could not locate the home folder.".to_string())?;
    tokio::task::spawn_blocking(move || {
        set_inventory_record_enabled_service(
            home_dir,
            project_roots,
            InventoryActionRequest {
                record_id,
                enabled,
                source_revision,
            },
        )
        .map_err(|error| error.to_string())
    })
    .await
    .map_err(|_| "The inventory action could not be completed.".to_string())?
}

fn project_roots(db: &State<'_, Arc<Mutex<Database>>>) -> Result<Vec<PathBuf>, String> {
    let database = db
        .lock()
        .map_err(|_| "Could not access the project list.".to_string())?;
    Ok(database
        .get_all_projects()
        .map_err(|_| "Could not load the project list.".to_string())?
        .into_iter()
        .map(|project| PathBuf::from(project.path))
        .collect())
}
