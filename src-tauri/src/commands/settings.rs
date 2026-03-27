use std::collections::HashMap;

use crate::errors::AppError;
use crate::storage::db::AppDb;
use crate::storage::queries;

#[tauri::command]
pub async fn get_settings(
    db: tauri::State<'_, AppDb>,
) -> Result<HashMap<String, String>, AppError> {
    let conn =
        db.0.lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;
    Ok(queries::get_all_settings(&conn)?)
}

#[tauri::command]
pub async fn update_setting(
    key: String,
    value: String,
    db: tauri::State<'_, AppDb>,
) -> Result<(), AppError> {
    let conn =
        db.0.lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;
    Ok(queries::upsert_setting(&conn, &key, &value)?)
}

#[cfg(test)]
mod tests {
    use crate::storage::db::init_db_in_memory;
    use crate::storage::queries;

    #[test]
    fn test_settings_round_trip() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        queries::upsert_setting(&conn, "preferred_provider", "claude").unwrap();
        let val = queries::get_setting(&conn, "preferred_provider").unwrap();
        assert_eq!(val, Some("claude".to_string()));
    }

    #[test]
    fn test_get_all_settings_empty() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        let all = queries::get_all_settings(&conn).unwrap();
        assert!(all.is_empty());
    }

    #[test]
    fn test_upsert_overwrites() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        queries::upsert_setting(&conn, "key", "v1").unwrap();
        queries::upsert_setting(&conn, "key", "v2").unwrap();
        assert_eq!(
            queries::get_setting(&conn, "key").unwrap(),
            Some("v2".into())
        );
    }

    #[test]
    fn test_get_all_settings_multiple() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        queries::upsert_setting(&conn, "a", "1").unwrap();
        queries::upsert_setting(&conn, "b", "2").unwrap();
        let all = queries::get_all_settings(&conn).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all.get("a"), Some(&"1".to_string()));
        assert_eq!(all.get("b"), Some(&"2".to_string()));
    }
}
