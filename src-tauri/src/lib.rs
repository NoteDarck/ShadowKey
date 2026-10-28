use iwcore::{PasswordOptions, Wallet};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
#[cfg(mobile)]
use tauri::Manager;
use tauri::State;

const VAULT_FILE: &str = "nswallet.dat";
const PORTABLE_FOLDER: &str = "ShadowKeyVault";
const MIN_MASTER_LEN: usize = 8;
const MAX_MASTER_LEN: usize = 1024;

struct VaultSession {
    wallet: Wallet,
    folder: PathBuf,
}

#[derive(Default)]
struct AppState {
    session: Mutex<Option<VaultSession>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EntrySummary {
    id: String,
    name: String,
    username: String,
    has_password: bool,
    updated_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EntryDetails {
    id: String,
    name: String,
    username: String,
    notes: String,
    has_password: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VaultInfo {
    folder: String,
    unlocked: bool,
    entries: Vec<EntrySummary>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FolderInspection {
    folder: String,
    has_vault: bool,
    size_bytes: u64,
}

fn wallet_err(error: impl std::fmt::Display) -> String {
    format!("Erro no cofre: {error}")
}

fn normalize_vault_folder(input: &str) -> Result<PathBuf, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Escolha uma pasta para o cofre.".into());
    }

    let path = PathBuf::from(trimmed);
    let folder = if path.file_name().and_then(|name| name.to_str()) == Some(VAULT_FILE) {
        path.parent()
            .ok_or_else(|| "Caminho do cofre inválido.".to_string())?
            .to_path_buf()
    } else {
        path
    };

    if folder.exists() && !folder.is_dir() {
        return Err("O caminho escolhido não é uma pasta.".into());
    }

    Ok(folder)
}

fn validate_new_master_password(password: &str) -> Result<(), String> {
    let length = password.chars().count();
    if length < MIN_MASTER_LEN {
        return Err(format!(
            "A senha mestra precisa ter pelo menos {MIN_MASTER_LEN} caracteres."
        ));
    }
    if length > MAX_MASTER_LEN {
        return Err("A senha mestra é longa demais.".into());
    }
    Ok(())
}

fn entries_summary(wallet: &mut Wallet) -> Result<Vec<EntrySummary>, String> {
    let items = wallet.get_items().map_err(wallet_err)?.to_vec();
    let mut entries = Vec::new();

    for item in items.into_iter().filter(|item| !item.folder) {
        let fields = wallet
            .get_fields_by_item(&item.item_id)
            .map_err(wallet_err)?;
        let username = fields
            .iter()
            .find(|field| field.field_type == "USER")
            .map(|field| field.value.clone())
            .unwrap_or_default();
        let has_password = fields
            .iter()
            .any(|field| field.field_type == "PASS" && !field.value.is_empty());

        entries.push(EntrySummary {
            id: item.item_id,
            name: item.name,
            username,
            has_password,
            updated_at: item.change_timestamp.to_rfc3339(),
        });
    }

    entries.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.username.to_lowercase().cmp(&b.username.to_lowercase()))
    });

    Ok(entries)
}

fn with_session<T>(
    state: &State<'_, AppState>,
    action: impl FnOnce(&mut VaultSession) -> Result<T, String>,
) -> Result<T, String> {
    let mut guard = state
        .session
        .lock()
        .map_err(|_| "Estado interno do cofre indisponível.".to_string())?;
    let session = guard
        .as_mut()
        .ok_or_else(|| "O cofre está bloqueado.".to_string())?;
    action(session)
}

fn replace_session(state: &State<'_, AppState>, session: VaultSession) -> Result<(), String> {
    let mut guard = state
        .session
        .lock()
        .map_err(|_| "Estado interno do cofre indisponível.".to_string())?;
    if let Some(mut previous) = guard.take() {
        previous.wallet.close();
    }
    *guard = Some(session);
    Ok(())
}

fn set_field(
    wallet: &mut Wallet,
    item_id: &str,
    field_type: &str,
    value: &str,
) -> Result<(), String> {
    let fields = wallet
        .get_fields_by_item(item_id)
        .map_err(wallet_err)?;

    if let Some(field) = fields.iter().find(|field| field.field_type == field_type) {
        if value.is_empty() {
            wallet
                .delete_field(item_id, &field.field_id)
                .map_err(wallet_err)?;
        } else if field.value != value {
            wallet
                .update_field(&field.field_id, value, None)
                .map_err(wallet_err)?;
        }
    } else if !value.is_empty() {
        wallet
            .add_field(item_id, field_type, value, None)
            .map_err(wallet_err)?;
    }

    Ok(())
}

fn portable_base_folder() -> Result<PathBuf, String> {
    if let Ok(appimage) = std::env::var("APPIMAGE") {
        if !appimage.trim().is_empty() {
            return PathBuf::from(appimage)
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| "Não foi possível localizar a pasta do aplicativo.".to_string());
        }
    }

    std::env::current_exe()
        .map_err(|error| format!("Não foi possível localizar o aplicativo: {error}"))?
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "Não foi possível localizar a pasta do aplicativo.".to_string())
}

#[tauri::command]
fn inspect_vault(path: String) -> Result<FolderInspection, String> {
    let folder = normalize_vault_folder(&path)?;
    let database = folder.join(VAULT_FILE);
    let has_vault = database.is_file();
    let size_bytes = if has_vault {
        std::fs::metadata(&database).map(|meta| meta.len()).unwrap_or(0)
    } else {
        0
    };

    Ok(FolderInspection {
        folder: folder.to_string_lossy().into_owned(),
        has_vault,
        size_bytes,
    })
}

#[tauri::command]
fn is_mobile() -> bool {
    cfg!(mobile)
}

#[tauri::command]
fn suggest_portable_folder(app: tauri::AppHandle) -> Result<String, String> {
    #[cfg(mobile)]
    {
        let folder = app
            .path()
            .app_data_dir()
            .map_err(|error| format!("Não foi possível localizar o armazenamento do aplicativo: {error}"))?
            .join(PORTABLE_FOLDER);
        Ok(folder.to_string_lossy().into_owned())
    }

    #[cfg(not(mobile))]
    {
        let _ = app;
        let folder = portable_base_folder()?.join(PORTABLE_FOLDER);
        Ok(folder.to_string_lossy().into_owned())
    }
}

#[tauri::command(async)]
fn create_vault(
    path: String,
    master_password: String,
    state: State<'_, AppState>,
) -> Result<VaultInfo, String> {
    validate_new_master_password(&master_password)?;

    let folder = normalize_vault_folder(&path)?;
    std::fs::create_dir_all(&folder)
        .map_err(|error| format!("Não foi possível criar a pasta do cofre: {error}"))?;

    let database = folder.join(VAULT_FILE);
    if database.exists() {
        return Err("Já existe um cofre nesta pasta. Use a opção Abrir cofre.".into());
    }

    let wallet = Wallet::create(&folder, &master_password, "pt").map_err(wallet_err)?;
    let folder = std::fs::canonicalize(&folder).unwrap_or(folder);

    replace_session(
        &state,
        VaultSession {
            wallet,
            folder: folder.clone(),
        },
    )?;

    Ok(VaultInfo {
        folder: folder.to_string_lossy().into_owned(),
        unlocked: true,
        entries: Vec::new(),
    })
}

#[tauri::command(async)]
fn open_vault(
    path: String,
    master_password: String,
    state: State<'_, AppState>,
) -> Result<VaultInfo, String> {
    let folder = normalize_vault_folder(&path)?;
    let database = folder.join(VAULT_FILE);
    if !database.is_file() {
        return Err("Nenhum cofre encontrado nesta pasta. Crie um novo cofre primeiro.".into());
    }

    let mut wallet = Wallet::open(&folder).map_err(wallet_err)?;
    if !wallet.unlock(&master_password).map_err(wallet_err)? {
        wallet.close();
        return Err("Senha mestra incorreta.".into());
    }

    let entries = entries_summary(&mut wallet)?;
    let folder = std::fs::canonicalize(&folder).unwrap_or(folder);

    replace_session(
        &state,
        VaultSession {
            wallet,
            folder: folder.clone(),
        },
    )?;

    Ok(VaultInfo {
        folder: folder.to_string_lossy().into_owned(),
        unlocked: true,
        entries,
    })
}

#[tauri::command]
fn lock_vault(state: State<'_, AppState>) -> Result<(), String> {
    let mut guard = state
        .session
        .lock()
        .map_err(|_| "Estado interno do cofre indisponível.".to_string())?;
    if let Some(mut session) = guard.take() {
        session.wallet.close();
    }
    Ok(())
}

#[tauri::command]
fn get_status(state: State<'_, AppState>) -> Result<VaultInfo, String> {
    let mut guard = state
        .session
        .lock()
        .map_err(|_| "Estado interno do cofre indisponível.".to_string())?;

    match guard.as_mut() {
        Some(session) => {
            let entries = entries_summary(&mut session.wallet)?;
            Ok(VaultInfo {
                folder: session.folder.to_string_lossy().into_owned(),
                unlocked: true,
                entries,
            })
        }
        None => Ok(VaultInfo {
            folder: String::new(),
            unlocked: false,
            entries: Vec::new(),
        }),
    }
}

#[tauri::command]
fn list_entries(state: State<'_, AppState>) -> Result<Vec<EntrySummary>, String> {
    with_session(&state, |session| entries_summary(&mut session.wallet))
}

#[tauri::command]
fn get_entry(id: String, state: State<'_, AppState>) -> Result<EntryDetails, String> {
    with_session(&state, |session| {
        let item = session
            .wallet
            .get_item(&id)
            .map_err(wallet_err)?
            .ok_or_else(|| "Entrada não encontrada.".to_string())?;

        if item.folder {
            return Err("Esta entrada não é uma senha.".into());
        }

        let fields = session
            .wallet
            .get_fields_by_item(&id)
            .map_err(wallet_err)?;
        let username = fields
            .iter()
            .find(|field| field.field_type == "USER")
            .map(|field| field.value.clone())
            .unwrap_or_default();
        let notes = fields
            .iter()
            .find(|field| field.field_type == "NOTE")
            .map(|field| field.value.clone())
            .unwrap_or_default();
        let has_password = fields
            .iter()
            .any(|field| field.field_type == "PASS" && !field.value.is_empty());

        Ok(EntryDetails {
            id: item.item_id,
            name: item.name,
            username,
            notes,
            has_password,
        })
    })
}

#[tauri::command]
fn get_secret(id: String, field: String, state: State<'_, AppState>) -> Result<String, String> {
    let field_type = match field.as_str() {
        "USER" | "PASS" | "NOTE" => field,
        _ => return Err("Campo inválido.".into()),
    };

    with_session(&state, |session| {
        let fields = session
            .wallet
            .get_fields_by_item(&id)
            .map_err(wallet_err)?;
        Ok(fields
            .iter()
            .find(|item| item.field_type == field_type)
            .map(|item| item.value.clone())
            .unwrap_or_default())
    })
}

#[tauri::command]
fn add_entry(
    service: String,
    username: String,
    password: String,
    notes: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let service = service.trim();
    if service.is_empty() {
        return Err("Informe o serviço ou site.".into());
    }
    if password.is_empty() {
        return Err("Informe a senha.".into());
    }

    with_session(&state, |session| {
        let item_id = session
            .wallet
            .add_item(service, "pass", false, None)
            .map_err(wallet_err)?;

        let result = (|| -> Result<(), String> {
            if !username.is_empty() {
                session
                    .wallet
                    .add_field(&item_id, "USER", &username, None)
                    .map_err(wallet_err)?;
            }
            session
                .wallet
                .add_field(&item_id, "PASS", &password, None)
                .map_err(wallet_err)?;
            if !notes.trim().is_empty() {
                session
                    .wallet
                    .add_field(&item_id, "NOTE", &notes, None)
                    .map_err(wallet_err)?;
            }
            Ok(())
        })();

        if let Err(error) = result {
            let _ = session.wallet.delete_item(&item_id);
            return Err(error);
        }

        Ok(item_id)
    })
}

#[tauri::command]
fn update_entry(
    id: String,
    service: String,
    username: String,
    password: String,
    notes: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let service = service.trim();
    if service.is_empty() {
        return Err("Informe o serviço ou site.".into());
    }
    if password.is_empty() {
        return Err("Informe a senha.".into());
    }

    with_session(&state, |session| {
        let item = session
            .wallet
            .get_item(&id)
            .map_err(wallet_err)?
            .ok_or_else(|| "Entrada não encontrada.".to_string())?;

        if item.folder {
            return Err("Esta entrada não é uma senha.".into());
        }

        session
            .wallet
            .update_item_name(&id, service)
            .map_err(wallet_err)?;
        set_field(&mut session.wallet, &id, "USER", &username)?;
        set_field(&mut session.wallet, &id, "PASS", &password)?;
        set_field(&mut session.wallet, &id, "NOTE", &notes)?;
        Ok(())
    })
}

#[tauri::command]
fn delete_entry(id: String, state: State<'_, AppState>) -> Result<(), String> {
    with_session(&state, |session| {
        session.wallet.delete_item(&id).map_err(wallet_err)
    })
}

#[tauri::command(async)]
fn change_master_password(
    current_password: String,
    new_password: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    validate_new_master_password(&new_password)?;

    with_session(&state, |session| {
        if !session
            .wallet
            .check_password(&current_password)
            .map_err(wallet_err)?
        {
            return Err("Senha atual incorreta.".into());
        }

        if !session
            .wallet
            .change_password(&new_password)
            .map_err(wallet_err)?
        {
            return Err("Não foi possível alterar a senha mestra.".into());
        }

        Ok(())
    })
}

#[tauri::command]
fn generate_password(
    length: usize,
    lowercase: bool,
    uppercase: bool,
    digits: bool,
    special: bool,
    avoid_ambiguous: bool,
) -> Result<String, String> {
    if ![lowercase, uppercase, digits, special]
        .iter()
        .any(|enabled| *enabled)
    {
        return Err("Selecione pelo menos um tipo de caractere.".into());
    }

    let options = PasswordOptions {
        lowercase,
        uppercase,
        digits,
        special,
        avoid_ambiguous,
        length: length.clamp(8, 128),
    };

    Ok(iwcore::generate_password(&options))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![
            inspect_vault,
            is_mobile,
            suggest_portable_folder,
            create_vault,
            open_vault,
            lock_vault,
            get_status,
            list_entries,
            get_entry,
            get_secret,
            add_entry,
            update_entry,
            delete_entry,
            change_master_password,
            generate_password
        ])
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o ShadowKey");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn normalizes_database_file_to_parent_folder() {
        let folder =
            normalize_vault_folder("/tmp/pendrive/ShadowKeyVault/nswallet.dat").unwrap();
        assert_eq!(folder, PathBuf::from("/tmp/pendrive/ShadowKeyVault"));
        assert!(normalize_vault_folder("   ").is_err());
        assert!(validate_new_master_password("curta").is_err());
        assert!(validate_new_master_password("senha-boa-123").is_ok());
    }

    #[test]
    fn vault_roundtrip_with_authentication() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let folder = std::env::temp_dir().join(format!(
            "shadowkey-test-{}-{unique}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&folder);

        let mut wallet = Wallet::create(&folder, "senha-mestra-123", "pt").unwrap();
        let item_id = wallet
            .add_item("Gmail", "pass", false, None)
            .unwrap();
        wallet
            .add_field(&item_id, "USER", "usuario@exemplo.com", None)
            .unwrap();
        wallet
            .add_field(&item_id, "PASS", "segredo-super-forte", None)
            .unwrap();

        let entries = entries_summary(&mut wallet).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "Gmail");
        assert_eq!(entries[0].username, "usuario@exemplo.com");
        assert!(entries[0].has_password);
        wallet.close();

        let mut wallet = Wallet::open(&folder).unwrap();
        assert!(wallet.unlock("senha-mestra-123").unwrap());
        let fields = wallet.get_fields_by_item(&item_id).unwrap();
        assert_eq!(
            fields
                .iter()
                .find(|field| field.field_type == "PASS")
                .unwrap()
                .value,
            "segredo-super-forte"
        );
        wallet.close();

        let mut wallet = Wallet::open(&folder).unwrap();
        assert!(!wallet.unlock("senha-errada").unwrap());
        wallet.close();

        let _ = std::fs::remove_dir_all(&folder);
    }
}
