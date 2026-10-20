use iwcore::Wallet;
use std::path::Path;
use tauri::Manager;

// Comando: Criar um novo cofre (chamado na primeira vez que o usuário usa)
#[tauri::command]
fn create_vault(path: String, master_password: String) -> Result<String, String> {
    // "pt" representa o idioma, iwcore suporta múltiplos idiomas
    match Wallet::create(Path::new(&path), &master_password, "pt") {
        Ok(_) => Ok(format!("Cofre criado com sucesso: {}", path)),
        Err(e) => Err(format!("Falha ao criar: {}", e)),
    }
}

// Comando: Desbloquear o cofre e listar todas as entradas (para interface simples, retorna diretamente)
#[tauri::command]
fn unlock_and_list(path: String, master_password: String) -> Result<Vec<serde_json::Value>, String> {
    let mut wallet = Wallet::open(Path::new(&path)).map_err(|e| e.to_string())?;
    
    if !wallet.unlock(&master_password).map_err(|e| e.to_string())? {
        return Err("Senha mestra incorreta".into());
    }

    let items = wallet.get_items().map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    
    for item in items {
        // Para simplificar, aqui retornamos apenas o nome e o ID do item; os campos reais precisam ser obtidos via get_fields
        result.push(serde_json::json!({
            "id": item.item_id,
            "name": item.name
        }));
    }
    
    wallet.close(); // Limpa a memória
    Ok(result)
}

// Comando: Adicionar uma nova senha
#[tauri::command]
fn add_credential(
    path: String,
    master_password: String,
    service: String,
    username: String,
    password: String,
) -> Result<(), String> {
    let mut wallet = Wallet::open(Path::new(&path)).map_err(|e| e.to_string())?;
    wallet.unlock(&master_password).map_err(|e| e.to_string())?;

    // Cria um item do tipo "document"
    let item_id = wallet.add_item(&service, "document", false, None)
        .map_err(|e| e.to_string())?;
    
    // Adiciona campos (tipo USER e PASS são embutidos no iwcore)
    wallet.add_field(&item_id, "USER", &username, None)
        .map_err(|e| e.to_string())?;
    wallet.add_field(&item_id, "PASS", &password, None)
        .map_err(|e| e.to_string())?;

    wallet.close();
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init()) // Inicializa o plugin de diálogo
        .invoke_handler(tauri::generate_handler![
            create_vault,
            unlock_and_list,
            add_credential
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}