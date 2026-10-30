use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use data_encoding::BASE32_NOPAD;
use iwcore::{PasswordOptions, Wallet};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
#[cfg(mobile)]
use tauri::Manager;
use tauri::State;
use totp_rs::{Builder, Totp};

const VAULT_FILE: &str = "nswallet.dat";
const PORTABLE_FOLDER: &str = "ShadowKeyVault";
const MIN_MASTER_LEN: usize = 8;
const MAX_MASTER_LEN: usize = 1024;
const MAX_IMPORT_ROWS: usize = 10_000;

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
    has_totp: bool,
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
    has_totp: bool,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StorageOption {
    path: String,
    label: String,
    kind: String,
    available: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportResult {
    imported: usize,
    skipped: usize,
    errors: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TotpCode {
    code: String,
    seconds_remaining: u64,
    period: u64,
    digits: u8,
}

struct CsvEntry {
    line: usize,
    name: String,
    username: String,
    password: String,
    notes: String,
    totp: String,
}

struct ParsedCsv {
    entries: Vec<CsvEntry>,
    skipped: usize,
    errors: Vec<String>,
    warnings: Vec<String>,
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
        let has_totp = fields
            .iter()
            .any(|field| field.field_type == "TOTP" && !field.value.is_empty());

        entries.push(EntrySummary {
            id: item.item_id,
            name: item.name,
            username,
            has_password,
            has_totp,
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
    let fields = wallet.get_fields_by_item(item_id).map_err(wallet_err)?;

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

fn validate_totp(totp: &Totp) -> Result<(), String> {
    if totp.step() == 0 {
        return Err("Período do código 2FA inválido.".into());
    }
    if !(6..=8).contains(&totp.digits()) {
        return Err("Quantidade de dígitos do código 2FA inválida.".into());
    }
    if totp.secret().as_bytes().is_empty() {
        return Err("Segredo 2FA vazio.".into());
    }
    Ok(())
}

fn normalize_totp(input: &str, service: &str) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(String::new());
    }

    if input.starts_with("otpauth://") {
        let totp = Totp::from_url_unchecked(input)
            .map_err(|error| format!("Código 2FA inválido: {error}"))?;
        validate_totp(&totp)?;
        return totp
            .to_url()
            .map_err(|error| format!("Código 2FA inválido: {error}"));
    }

    if input.starts_with("otpauth-migration://") {
        return Err("Use a importação de arquivo para códigos do Google Authenticator.".into());
    }

    let secret = input
        .chars()
        .filter(|character| !character.is_whitespace() && *character != '-')
        .collect::<String>()
        .trim_end_matches('=')
        .to_uppercase();
    if secret.is_empty() {
        return Ok(String::new());
    }

    let bytes = BASE32_NOPAD
        .decode(secret.as_bytes())
        .map_err(|_| "Segredo 2FA Base32 inválido.".to_string())?;
    if bytes.is_empty() {
        return Err("Segredo 2FA vazio.".into());
    }

    let account = service.trim().replace(':', " ");
    let account = if account.is_empty() {
        "Conta 2FA".to_string()
    } else {
        account
    };
    let totp = Builder::new()
        .with_secret(bytes)
        .with_account_name(account)
        .build_noncompliant();
    validate_totp(&totp)?;

    totp.to_url()
        .map_err(|error| format!("Segredo 2FA inválido: {error}"))
}

fn totp_from_stored(value: &str, service: &str) -> Result<Totp, String> {
    let normalized = normalize_totp(value, service)?;
    if normalized.is_empty() {
        return Err("Esta entrada não tem 2FA.".into());
    }
    let totp = Totp::from_url_unchecked(&normalized)
        .map_err(|error| format!("Código 2FA inválido: {error}"))?;
    validate_totp(&totp)?;
    Ok(totp)
}

fn add_entry_to_wallet(
    wallet: &mut Wallet,
    service: &str,
    username: &str,
    password: &str,
    notes: &str,
    totp: &str,
) -> Result<String, String> {
    let item_id = wallet
        .add_item(service, "pass", false, None)
        .map_err(wallet_err)?;

    let result = (|| -> Result<(), String> {
        if !username.is_empty() {
            wallet
                .add_field(&item_id, "USER", username, None)
                .map_err(wallet_err)?;
        }
        if !password.is_empty() {
            wallet
                .add_field(&item_id, "PASS", password, None)
                .map_err(wallet_err)?;
        }
        if !totp.is_empty() {
            wallet
                .add_field(&item_id, "TOTP", totp, None)
                .map_err(wallet_err)?;
        }
        if !notes.trim().is_empty() {
            wallet
                .add_field(&item_id, "NOTE", notes, None)
                .map_err(wallet_err)?;
        }
        Ok(())
    })();

    if let Err(error) = result {
        let _ = wallet.delete_item(&item_id);
        return Err(error);
    }

    Ok(item_id)
}

fn ascii_fold(value: char) -> char {
    match value {
        'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        'ñ' => 'n',
        _ => value,
    }
}

fn normalize_header(value: &str) -> String {
    value
        .trim_start_matches('\u{feff}')
        .chars()
        .filter(|character| !matches!(character, ' ' | '_' | '-' | '.'))
        .flat_map(char::to_lowercase)
        .map(ascii_fold)
        .collect()
}

fn find_header(headers: &HashMap<String, usize>, aliases: &[&str]) -> Option<usize> {
    aliases
        .iter()
        .find_map(|alias| headers.get(*alias).copied())
}

fn detect_csv_delimiter(csv: &str) -> u8 {
    let first_line = csv.lines().next().unwrap_or_default();
    let candidates = [
        (b',', first_line.matches(',').count()),
        (b';', first_line.matches(';').count()),
        (b'\t', first_line.matches('\t').count()),
    ];

    candidates
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(delimiter, _)| delimiter)
        .unwrap_or(b',')
}

fn csv_field(record: &csv::StringRecord, index: Option<usize>) -> &str {
    index
        .and_then(|position| record.get(position))
        .unwrap_or_default()
        .trim()
}

fn service_from_url(url: &str) -> String {
    let value = url.trim();
    if value.is_empty() {
        return String::new();
    }

    let without_scheme = value.split("://").nth(1).unwrap_or(value);
    let host = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(without_scheme)
        .rsplit('@')
        .next()
        .unwrap_or(without_scheme)
        .split(':')
        .next()
        .unwrap_or(without_scheme);

    host.trim_start_matches("www.")
        .trim_end_matches('.')
        .to_string()
}

fn read_varint(bytes: &[u8], position: &mut usize) -> Result<u64, String> {
    let mut value = 0u64;
    let mut shift = 0u32;
    loop {
        let byte = *bytes
            .get(*position)
            .ok_or_else(|| "Payload 2FA truncado.".to_string())?;
        *position += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
        if shift >= 64 {
            return Err("Payload 2FA inválido.".into());
        }
    }
}

fn read_length_delimited<'a>(bytes: &'a [u8], position: &mut usize) -> Result<&'a [u8], String> {
    let length = usize::try_from(read_varint(bytes, position)?)
        .map_err(|_| "Payload 2FA inválido.".to_string())?;
    let end = (*position)
        .checked_add(length)
        .ok_or_else(|| "Payload 2FA inválido.".to_string())?;
    let value = bytes
        .get(*position..end)
        .ok_or_else(|| "Payload 2FA truncado.".to_string())?;
    *position = end;
    Ok(value)
}

fn skip_protobuf_field(bytes: &[u8], position: &mut usize, wire_type: u8) -> Result<(), String> {
    match wire_type {
        0 => {
            read_varint(bytes, position)?;
        }
        1 => {
            *position = position
                .checked_add(8)
                .ok_or_else(|| "Payload 2FA inválido.".to_string())?;
        }
        2 => {
            read_length_delimited(bytes, position)?;
        }
        5 => {
            *position = position
                .checked_add(4)
                .ok_or_else(|| "Payload 2FA inválido.".to_string())?;
        }
        _ => return Err("Payload 2FA com formato desconhecido.".into()),
    }

    if *position > bytes.len() {
        return Err("Payload 2FA truncado.".into());
    }
    Ok(())
}

#[derive(Default)]
struct MigrationOtp {
    secret: Vec<u8>,
    name: String,
    issuer: String,
    algorithm: i32,
    digits: i32,
    otp_type: i32,
}

fn parse_migration_otp(bytes: &[u8]) -> Result<MigrationOtp, String> {
    let mut otp = MigrationOtp::default();
    let mut position = 0;

    while position < bytes.len() {
        let key = read_varint(bytes, &mut position)?;
        let field = key >> 3;
        let wire_type = (key & 7) as u8;

        match (field, wire_type) {
            (1, 2) => otp.secret = read_length_delimited(bytes, &mut position)?.to_vec(),
            (2, 2) => {
                otp.name = String::from_utf8_lossy(read_length_delimited(bytes, &mut position)?)
                    .to_string()
            }
            (3, 2) => {
                otp.issuer = String::from_utf8_lossy(read_length_delimited(bytes, &mut position)?)
                    .to_string()
            }
            (4, 0) => otp.algorithm = read_varint(bytes, &mut position)? as i32,
            (5, 0) => otp.digits = read_varint(bytes, &mut position)? as i32,
            (6, 0) => otp.otp_type = read_varint(bytes, &mut position)? as i32,
            (7, 0) => {
                read_varint(bytes, &mut position)?;
            }
            (_, _) => skip_protobuf_field(bytes, &mut position, wire_type)?,
        }
    }

    Ok(otp)
}

fn parse_migration_payload(bytes: &[u8]) -> Result<Vec<MigrationOtp>, String> {
    let mut items = Vec::new();
    let mut position = 0;

    while position < bytes.len() {
        let key = read_varint(bytes, &mut position)?;
        let field = key >> 3;
        let wire_type = (key & 7) as u8;

        match (field, wire_type) {
            (1, 2) => items.push(parse_migration_otp(read_length_delimited(
                bytes,
                &mut position,
            )?)?),
            (_, _) => skip_protobuf_field(bytes, &mut position, wire_type)?,
        }
    }

    Ok(items)
}

fn parse_google_migration_uri(
    uri: &str,
    warnings: &mut Vec<String>,
) -> Result<Vec<CsvEntry>, String> {
    let payload = uri
        .split_once("data=")
        .map(|(_, value)| value)
        .ok_or_else(|| "QR do Google Authenticator sem dados.".to_string())?;
    let payload = payload.split('&').next().unwrap_or(payload);
    let decoded = urlencoding::decode(payload)
        .map_err(|_| "Payload do Google Authenticator inválido.".to_string())?;
    let decoded = decoded.trim().replace('-', "+").replace('_', "/");
    let mut padded = decoded;
    while padded.len() % 4 != 0 {
        padded.push('=');
    }
    let bytes = BASE64_STANDARD
        .decode(padded.as_bytes())
        .map_err(|_| "Payload do Google Authenticator não está em Base64.".to_string())?;

    let mut entries = Vec::new();
    for item in parse_migration_payload(&bytes)? {
        if item.otp_type == 1 {
            warnings.push("Uma conta HOTP foi ignorada; apenas TOTP é suportado.".into());
            continue;
        }
        if item.secret.is_empty() {
            warnings.push("Uma conta 2FA sem segredo foi ignorada.".into());
            continue;
        }

        let secret = BASE32_NOPAD.encode(&item.secret);
        let name = item.name.trim();
        let issuer = item.issuer.trim();
        if name.is_empty() && issuer.is_empty() {
            warnings.push("Uma conta 2FA sem nome foi ignorada.".into());
            continue;
        }

        let algorithm = match item.algorithm {
            0 | 1 => "SHA1",
            2 => "SHA256",
            3 => "SHA512",
            _ => {
                warnings.push("Uma conta 2FA com algoritmo não suportado foi ignorada.".into());
                continue;
            }
        };
        let digits = match item.digits {
            0 | 1 => 6,
            2 => 8,
            _ => {
                warnings
                    .push("Uma conta 2FA com quantidade de dígitos inválida foi ignorada.".into());
                continue;
            }
        };

        let label = if issuer.is_empty() {
            urlencoding::encode(name).to_string()
        } else if name.is_empty() {
            urlencoding::encode(issuer).to_string()
        } else {
            format!(
                "{}:{}",
                urlencoding::encode(issuer),
                urlencoding::encode(name)
            )
        };
        let mut url = format!("otpauth://totp/{label}?secret={secret}");
        if !issuer.is_empty() {
            url.push_str("&issuer=");
            url.push_str(&urlencoding::encode(issuer));
        }
        if algorithm != "SHA1" {
            url.push_str("&algorithm=");
            url.push_str(algorithm);
        }
        if digits != 6 {
            url.push_str("&digits=");
            url.push_str(&digits.to_string());
        }

        let totp = Totp::from_url_unchecked(&url).map_err(|error| {
            format!(
                "Conta {}: {error}",
                if issuer.is_empty() { name } else { issuer }
            )
        })?;
        validate_totp(&totp)?;
        let normalized = totp
            .to_url()
            .map_err(|error| format!("Conta {}: {error}", totp.account_name()))?;
        let entry_name = totp.issuer().unwrap_or(totp.account_name()).to_string();
        let entry_username = if totp.issuer().is_some() {
            totp.account_name().to_string()
        } else {
            String::new()
        };

        entries.push(CsvEntry {
            line: 0,
            name: entry_name,
            username: entry_username,
            password: String::new(),
            notes: "Importado do Google Authenticator.".into(),
            totp: normalized,
        });
    }

    Ok(entries)
}

fn parse_otpauth_uri(uri: &str) -> Result<CsvEntry, String> {
    let totp =
        Totp::from_url_unchecked(uri).map_err(|error| format!("Código 2FA inválido: {error}"))?;
    validate_totp(&totp)?;
    let name = totp.issuer().unwrap_or(totp.account_name()).trim();
    if name.is_empty() {
        return Err("Conta 2FA sem nome.".into());
    }
    let normalized = totp
        .to_url()
        .map_err(|error| format!("Código 2FA inválido: {error}"))?;

    Ok(CsvEntry {
        line: 0,
        name: name.to_string(),
        username: if totp.issuer().is_some() {
            totp.account_name().to_string()
        } else {
            String::new()
        },
        password: String::new(),
        notes: "Importado do autenticador.".into(),
        totp: normalized,
    })
}

fn parse_otp_uris(data: &str) -> ParsedCsv {
    let mut parsed = ParsedCsv {
        entries: Vec::new(),
        skipped: 0,
        errors: Vec::new(),
        warnings: Vec::new(),
    };

    for (index, line) in data.lines().enumerate() {
        let line = line.trim();
        if line.starts_with("otpauth-migration://") {
            match parse_google_migration_uri(line, &mut parsed.warnings) {
                Ok(entries) => parsed.entries.extend(entries),
                Err(error) => {
                    parsed.skipped += 1;
                    parsed.errors.push(format!("Linha {}: {error}", index + 1));
                }
            }
        } else if line.starts_with("otpauth://") {
            match parse_otpauth_uri(line) {
                Ok(entry) => parsed.entries.push(entry),
                Err(error) => {
                    parsed.skipped += 1;
                    parsed.errors.push(format!("Linha {}: {error}", index + 1));
                }
            }
        }

        if parsed.entries.len() >= MAX_IMPORT_ROWS {
            parsed.errors.push(format!(
                "Limite de {MAX_IMPORT_ROWS} entradas atingido; o restante foi ignorado."
            ));
            break;
        }
    }

    parsed
}

fn looks_like_csv(data: &str) -> bool {
    let first_line = data
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default();
    let delimiter = detect_csv_delimiter(data);
    let headers: Vec<String> = first_line
        .split(delimiter as char)
        .map(normalize_header)
        .collect();

    headers.iter().any(|header| {
        matches!(
            header.as_str(),
            "name"
                | "title"
                | "service"
                | "servico"
                | "nome"
                | "url"
                | "loginuri"
                | "loginurl"
                | "username"
                | "usuario"
                | "password"
                | "senha"
                | "loginpassword"
                | "logintotp"
                | "totp"
                | "otp"
                | "otpauth"
        )
    })
}

fn parse_vault_data(data: &str) -> Result<ParsedCsv, String> {
    if looks_like_csv(data) {
        return parse_password_csv(data);
    }

    let parsed = parse_otp_uris(data);
    if parsed.entries.is_empty() && parsed.errors.is_empty() && parsed.warnings.is_empty() {
        return Err("Nenhum dado de senha ou 2FA reconhecido.".into());
    }
    Ok(parsed)
}

fn parse_password_csv(csv: &str) -> Result<ParsedCsv, String> {
    let delimiter = detect_csv_delimiter(csv);
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(csv.as_bytes());

    let headers = reader
        .headers()
        .map_err(|error| format!("Cabeçalho CSV inválido: {error}"))?
        .clone();

    let mut header_map = HashMap::new();
    for (index, header) in headers.iter().enumerate() {
        let key = normalize_header(header);
        if !key.is_empty() {
            header_map.entry(key).or_insert(index);
        }
    }

    let name_index = find_header(
        &header_map,
        &["name", "title", "service", "servico", "nome"],
    );
    let url_index = find_header(
        &header_map,
        &[
            "url",
            "loginuri",
            "loginurl",
            "website",
            "webaddress",
            "site",
            "uri",
            "link",
        ],
    );
    let username_index = find_header(
        &header_map,
        &[
            "username",
            "loginusername",
            "user",
            "login",
            "usuario",
            "email",
            "userid",
        ],
    );
    let password_index = find_header(&header_map, &["password", "loginpassword", "pass", "senha"]);
    let totp_index = find_header(
        &header_map,
        &[
            "totp",
            "logintotp",
            "otp",
            "otpauth",
            "twofactor",
            "2fa",
            "codigo2fa",
            "segredo2fa",
            "authenticator",
        ],
    );
    let note_index = find_header(
        &header_map,
        &[
            "note",
            "notes",
            "notas",
            "observacoes",
            "observacao",
            "comments",
            "comment",
            "extra",
        ],
    );

    if password_index.is_none() && totp_index.is_none() {
        return Err("O CSV precisa ter uma coluna de senha ou 2FA (password/totp).".into());
    }
    if name_index.is_none() && url_index.is_none() {
        return Err("O CSV precisa ter uma coluna de nome ou URL.".into());
    }

    let mut parsed = ParsedCsv {
        entries: Vec::new(),
        skipped: 0,
        errors: Vec::new(),
        warnings: Vec::new(),
    };

    for (index, record) in reader.records().enumerate() {
        let line = index + 2;
        if index >= MAX_IMPORT_ROWS {
            parsed.errors.push(format!(
                "Limite de {MAX_IMPORT_ROWS} linhas atingido; o restante foi ignorado."
            ));
            break;
        }

        let record = match record {
            Ok(record) => record,
            Err(error) => {
                parsed.skipped += 1;
                parsed
                    .errors
                    .push(format!("Linha {line}: CSV inválido ({error})."));
                continue;
            }
        };

        let password = csv_field(&record, password_index).to_string();
        let url = csv_field(&record, url_index).to_string();
        let mut name = csv_field(&record, name_index).to_string();
        if name.is_empty() {
            name = service_from_url(&url);
        }
        if name.is_empty() {
            parsed.skipped += 1;
            continue;
        }

        let raw_totp = csv_field(&record, totp_index).to_string();
        let mut totp = String::new();
        if !raw_totp.is_empty() {
            match normalize_totp(&raw_totp, &name) {
                Ok(value) => totp = value,
                Err(error) => parsed.warnings.push(format!("Linha {line}: {error}")),
            }
        }
        if password.is_empty() && totp.is_empty() {
            parsed.skipped += 1;
            continue;
        }

        let username = csv_field(&record, username_index).to_string();
        let note = csv_field(&record, note_index).to_string();
        let mut note_parts = Vec::new();
        if !url.is_empty() && !note.contains(&url) {
            note_parts.push(format!("URL: {url}"));
        }
        if !note.is_empty() {
            note_parts.push(note);
        }

        parsed.entries.push(CsvEntry {
            line,
            name,
            username,
            password,
            notes: note_parts.join("\n"),
            totp,
        });
    }

    Ok(parsed)
}

#[cfg(not(mobile))]
fn push_mount(
    path: PathBuf,
    seen: &mut std::collections::HashSet<PathBuf>,
    output: &mut Vec<(PathBuf, String)>,
) {
    if !path.is_dir() {
        return;
    }

    let path = std::fs::canonicalize(&path).unwrap_or(path);
    if !seen.insert(path.clone()) {
        return;
    }

    let label = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Unidade removível".to_string());
    output.push((path, format!("Unidade removível: {label}")));
}

#[cfg(not(mobile))]
fn collect_nested_mounts(
    base: &Path,
    seen: &mut std::collections::HashSet<PathBuf>,
    output: &mut Vec<(PathBuf, String)>,
) {
    let Ok(users) = std::fs::read_dir(base) else {
        return;
    };

    for user in users.flatten() {
        let user_path = user.path();
        if !user_path.is_dir() {
            continue;
        }
        let Ok(mounts) = std::fs::read_dir(&user_path) else {
            continue;
        };
        for mount in mounts.flatten() {
            push_mount(mount.path(), seen, output);
        }
    }
}

#[cfg(not(mobile))]
fn collect_direct_mounts(
    base: &Path,
    seen: &mut std::collections::HashSet<PathBuf>,
    output: &mut Vec<(PathBuf, String)>,
) {
    let Ok(entries) = std::fs::read_dir(base) else {
        return;
    };

    for entry in entries.flatten() {
        push_mount(entry.path(), seen, output);
    }
}

#[cfg(not(mobile))]
fn desktop_removable_mounts() -> Vec<(PathBuf, String)> {
    let mut output = Vec::new();
    let mut seen = std::collections::HashSet::new();

    #[cfg(target_os = "linux")]
    {
        collect_nested_mounts(Path::new("/run/media"), &mut seen, &mut output);
        collect_nested_mounts(Path::new("/media"), &mut seen, &mut output);
        collect_direct_mounts(Path::new("/mnt"), &mut seen, &mut output);
    }

    #[cfg(target_os = "macos")]
    {
        collect_direct_mounts(Path::new("/Volumes"), &mut seen, &mut output);
    }

    #[cfg(target_os = "windows")]
    {
        let system_drive = std::env::var("SystemDrive").unwrap_or_default();
        for letter in b'A'..=b'Z' {
            let path = PathBuf::from(format!("{}:\\", letter as char));
            if path
                .to_string_lossy()
                .trim_end_matches('\\')
                .eq_ignore_ascii_case(system_drive.trim_end_matches('\\'))
            {
                continue;
            }
            push_mount(path, &mut seen, &mut output);
        }
    }

    output.sort_by_key(|left| left.1.to_lowercase());
    output.truncate(12);
    output
}

#[cfg(target_os = "android")]
fn android_external_files_dirs() -> Result<Vec<PathBuf>, String> {
    use jni::objects::{JObject, JObjectArray, JString, JValue};
    use jni::JavaVM;

    let android = ndk_context::android_context();
    let vm = unsafe { JavaVM::from_raw(android.vm().cast()) }
        .map_err(|error| format!("Não foi possível acessar a JVM do Android: {error}"))?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|error| format!("Não foi possível acessar o ambiente Android: {error}"))?;
    let context = unsafe { JObject::from_raw(android.context().cast()) };
    let null = JObject::null();
    let files = env
        .call_method(
            &context,
            "getExternalFilesDirs",
            "(Ljava/lang/String;)[Ljava/io/File;",
            &[JValue::Object(&null)],
        )
        .map_err(|error| format!("Não foi possível consultar o armazenamento Android: {error}"))?
        .l()
        .map_err(|error| format!("Resposta inválida do armazenamento Android: {error}"))?;

    let files = JObjectArray::from(files);
    let length = env
        .get_array_length(&files)
        .map_err(|error| format!("Não foi possível listar o armazenamento Android: {error}"))?;
    let mut directories = Vec::new();

    for index in 0..length {
        let file = env
            .get_object_array_element(&files, index)
            .map_err(|error| format!("Não foi possível ler o armazenamento Android: {error}"))?;
        if file.as_raw().is_null() {
            continue;
        }

        let path = env
            .call_method(&file, "getAbsolutePath", "()Ljava/lang/String;", &[])
            .map_err(|error| format!("Não foi possível obter o caminho Android: {error}"))?
            .l()
            .map_err(|error| format!("Caminho Android inválido: {error}"))?;
        let path = String::from(
            env.get_string(&JString::from(path))
                .map_err(|error| format!("Caminho Android ilegível: {error}"))?,
        );

        if !path.trim().is_empty() {
            directories.push(PathBuf::from(path));
        }
    }

    Ok(directories)
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
        std::fs::metadata(&database)
            .map(|meta| meta.len())
            .unwrap_or(0)
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
            .map_err(|error| {
                format!("Não foi possível localizar o armazenamento do aplicativo: {error}")
            })?
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

#[tauri::command]
fn list_storage_options(app: tauri::AppHandle) -> Result<Vec<StorageOption>, String> {
    #[cfg(mobile)]
    {
        let private_folder = app
            .path()
            .app_data_dir()
            .map_err(|error| {
                format!("Não foi possível localizar o armazenamento do aplicativo: {error}")
            })?
            .join(PORTABLE_FOLDER);

        let mut options = vec![StorageOption {
            path: private_folder.to_string_lossy().into_owned(),
            label: "Armazenamento privado do aplicativo".into(),
            kind: "private".into(),
            available: true,
        }];

        #[cfg(target_os = "android")]
        if let Ok(directories) = android_external_files_dirs() {
            for directory in directories {
                let removable = !directory.to_string_lossy().contains("/emulated/");
                let folder = directory.join(PORTABLE_FOLDER);
                options.push(StorageOption {
                    path: folder.to_string_lossy().into_owned(),
                    label: if removable {
                        "Cartão SD (armazenamento externo)".into()
                    } else {
                        "Armazenamento externo (interno)".into()
                    },
                    kind: if removable {
                        "removable".into()
                    } else {
                        "external".into()
                    },
                    available: true,
                });
            }
        }

        Ok(options)
    }

    #[cfg(not(mobile))]
    {
        let _ = app;
        let portable_folder = portable_base_folder()?.join(PORTABLE_FOLDER);
        let mut options = vec![StorageOption {
            path: portable_folder.to_string_lossy().into_owned(),
            label: "Pasta ao lado do aplicativo (modo pendrive)".into(),
            kind: "portable".into(),
            available: true,
        }];

        for (mount, label) in desktop_removable_mounts() {
            options.push(StorageOption {
                path: mount.join(PORTABLE_FOLDER).to_string_lossy().into_owned(),
                label,
                kind: "removable".into(),
                available: true,
            });
        }

        Ok(options)
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

        let fields = session.wallet.get_fields_by_item(&id).map_err(wallet_err)?;
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
        let has_totp = fields
            .iter()
            .any(|field| field.field_type == "TOTP" && !field.value.is_empty());

        Ok(EntryDetails {
            id: item.item_id,
            name: item.name,
            username,
            notes,
            has_password,
            has_totp,
        })
    })
}

#[tauri::command]
fn get_secret(id: String, field: String, state: State<'_, AppState>) -> Result<String, String> {
    let field_type = match field.as_str() {
        "USER" | "PASS" | "NOTE" | "TOTP" => field,
        _ => return Err("Campo inválido.".into()),
    };

    with_session(&state, |session| {
        let fields = session.wallet.get_fields_by_item(&id).map_err(wallet_err)?;
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
    totp: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let service = service.trim();
    if service.is_empty() {
        return Err("Informe o serviço ou site.".into());
    }
    let totp = normalize_totp(&totp, service)?;
    if password.is_empty() && totp.is_empty() {
        return Err("Informe a senha ou o código 2FA.".into());
    }

    with_session(&state, |session| {
        add_entry_to_wallet(
            &mut session.wallet,
            service,
            &username,
            &password,
            &notes,
            &totp,
        )
    })
}

#[tauri::command]
fn get_totp(id: String, state: State<'_, AppState>) -> Result<TotpCode, String> {
    with_session(&state, |session| {
        let item = session
            .wallet
            .get_item(&id)
            .map_err(wallet_err)?
            .ok_or_else(|| "Entrada não encontrada.".to_string())?;
        if item.folder {
            return Err("Esta entrada não é uma senha.".into());
        }

        let fields = session.wallet.get_fields_by_item(&id).map_err(wallet_err)?;
        let value = fields
            .iter()
            .find(|field| field.field_type == "TOTP")
            .map(|field| field.value.as_str())
            .unwrap_or_default();
        let totp = totp_from_stored(value, &item.name)?;

        Ok(TotpCode {
            code: totp.generate_current().to_string(),
            seconds_remaining: totp.ttl(),
            period: totp.step(),
            digits: totp.digits(),
        })
    })
}

#[tauri::command(async)]
fn import_vault_data(data: String, state: State<'_, AppState>) -> Result<ImportResult, String> {
    let parsed = parse_vault_data(&data)?;

    with_session(&state, |session| {
        let mut imported = 0;
        let mut skipped = parsed.skipped;
        let mut errors = parsed.errors;
        let warnings = parsed.warnings;

        for entry in parsed.entries {
            match add_entry_to_wallet(
                &mut session.wallet,
                &entry.name,
                &entry.username,
                &entry.password,
                &entry.notes,
                &entry.totp,
            ) {
                Ok(_) => imported += 1,
                Err(error) => {
                    skipped += 1;
                    errors.push(format!("Linha {}: {error}", entry.line));
                }
            }
        }

        Ok(ImportResult {
            imported,
            skipped,
            errors,
            warnings,
        })
    })
}

#[tauri::command]
fn update_entry(
    id: String,
    service: String,
    username: String,
    password: String,
    notes: String,
    totp: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let service = service.trim();
    if service.is_empty() {
        return Err("Informe o serviço ou site.".into());
    }
    let totp = normalize_totp(&totp, service)?;
    if password.is_empty() && totp.is_empty() {
        return Err("Informe a senha ou o código 2FA.".into());
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
        set_field(&mut session.wallet, &id, "TOTP", &totp)?;
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
            list_storage_options,
            create_vault,
            open_vault,
            lock_vault,
            get_status,
            list_entries,
            get_entry,
            get_secret,
            get_totp,
            add_entry,
            import_vault_data,
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
        let folder = normalize_vault_folder("/tmp/pendrive/ShadowKeyVault/nswallet.dat").unwrap();
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
        let folder =
            std::env::temp_dir().join(format!("shadowkey-test-{}-{unique}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);

        let mut wallet = Wallet::create(&folder, "senha-mestra-123", "pt").unwrap();
        let item_id = wallet.add_item("Gmail", "pass", false, None).unwrap();
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

    #[test]
    fn parses_google_password_csv() {
        let csv = "\u{feff}name,url,username,password,note\n\
                   Gmail,https://accounts.google.com,user@gmail.com,segredo-123,Conta pessoal\n\
                   Sem Senha,https://example.com,user@example.com,,\n\
                   ,https://github.com/login,dev,outra-senha,\"Nota com, vírgula\"\n";

        let parsed = parse_password_csv(csv).unwrap();
        assert_eq!(parsed.entries.len(), 2);
        assert_eq!(parsed.skipped, 1);
        assert_eq!(parsed.entries[0].name, "Gmail");
        assert_eq!(parsed.entries[0].username, "user@gmail.com");
        assert_eq!(parsed.entries[0].password, "segredo-123");
        assert!(parsed.entries[0]
            .notes
            .contains("URL: https://accounts.google.com"));
        assert_eq!(parsed.entries[1].name, "github.com");
        assert!(parsed.entries[1].notes.contains("Nota com, vírgula"));
    }

    #[test]
    fn parses_bitwarden_and_semicolon_csv() {
        let bitwarden = "folder,favorite,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp\n\
                         ,0,login,Netflix,Plano familiar,,0,https://netflix.com,family@example.com,senha-netflix,\n";
        let parsed = parse_password_csv(bitwarden).unwrap();
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].name, "Netflix");
        assert_eq!(parsed.entries[0].username, "family@example.com");
        assert_eq!(parsed.entries[0].password, "senha-netflix");

        let semicolon = "nome;site;usuário;senha;observações\n\
                         Banco;https://banco.example;cliente;senha-banco;Conta principal\n";
        let parsed = parse_password_csv(semicolon).unwrap();
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].name, "Banco");
        assert_eq!(parsed.entries[0].username, "cliente");
        assert_eq!(parsed.entries[0].password, "senha-banco");
    }

    #[test]
    fn normalizes_raw_totp_secret() {
        let url = normalize_totp("jbswy3dp ehpk3pxp", "GitHub").unwrap();
        let totp = Totp::from_url_unchecked(&url).unwrap();
        assert_eq!(totp.account_name(), "GitHub");
        assert_eq!(totp.secret().to_base32(), "JBSWY3DPEHPK3PXP");
        assert_eq!(totp.generate_current().to_string().len(), 6);
        assert!(normalize_totp("segredo-invalido", "GitHub").is_err());
        assert!(normalize_totp(
            "otpauth://totp/GitHub?secret=JBSWY3DPEHPK3PXP&digits=9",
            "GitHub"
        )
        .is_err());
        assert!(normalize_totp(
            "otpauth://totp/GitHub?secret=JBSWY3DPEHPK3PXP&period=0",
            "GitHub"
        )
        .is_err());
    }

    #[test]
    fn generates_rfc6238_totp_code() {
        let totp = totp_from_stored(
            "otpauth://totp/Test?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&digits=8",
            "Test",
        )
        .unwrap();
        assert_eq!(totp.generate(59).to_string(), "94287082");
    }

    #[test]
    fn parses_csv_with_totp_only_entry() {
        let csv = "name,url,username,password,login_totp\n\
                   GitHub,https://github.com,dev,,otpauth://totp/GitHub:dev%40example.com?secret=JBSWY3DPEHPK3PXP&issuer=GitHub\n";
        let parsed = parse_vault_data(csv).unwrap();
        assert_eq!(parsed.entries.len(), 1);
        assert!(parsed.entries[0].password.is_empty());
        assert!(parsed.entries[0].totp.starts_with("otpauth://totp/"));
        let totp = Totp::from_url_unchecked(&parsed.entries[0].totp).unwrap();
        assert_eq!(totp.account_name(), "dev@example.com");
        assert_eq!(totp.issuer(), Some("GitHub"));
    }

    #[test]
    fn parses_google_authenticator_migration_payload() {
        let secret = BASE32_NOPAD.decode(b"JBSWY3DPEHPK3PXP").unwrap();
        let name = b"user@example.com";
        let issuer = b"Example";

        let mut otp = Vec::new();
        otp.extend([0x0a, secret.len() as u8]);
        otp.extend(&secret);
        otp.extend([0x12, name.len() as u8]);
        otp.extend(name);
        otp.extend([0x1a, issuer.len() as u8]);
        otp.extend(issuer);
        otp.extend([0x20, 0x01]);
        otp.extend([0x28, 0x01]);
        otp.extend([0x30, 0x02]);

        let mut payload = Vec::new();
        payload.extend([0x0a, otp.len() as u8]);
        payload.extend(&otp);
        let data = BASE64_STANDARD.encode(&payload);
        let uri = format!("otpauth-migration://offline?data={data}");

        let parsed = parse_vault_data(&uri).unwrap();
        assert!(
            parsed.errors.is_empty(),
            "erros: {:?}; avisos: {:?}",
            parsed.errors,
            parsed.warnings
        );
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].name, "Example");
        assert_eq!(parsed.entries[0].username, "user@example.com");
        assert!(parsed.entries[0].password.is_empty());
        let totp = Totp::from_url_unchecked(&parsed.entries[0].totp).unwrap();
        assert_eq!(totp.account_name(), "user@example.com");
        assert_eq!(totp.issuer(), Some("Example"));
        assert_eq!(totp.secret().to_base32(), "JBSWY3DPEHPK3PXP");
    }

    #[test]
    fn parses_multiple_otpauth_lines_from_text() {
        let data = "otpauth://totp/GitHub:dev%40example.com?secret=JBSWY3DPEHPK3PXP&issuer=GitHub\n\
                    otpauth://totp/Example:user%40example.com?secret=JBSWY3DPEHPK3PXP&issuer=Example\n";

        let parsed = parse_vault_data(data).unwrap();
        assert_eq!(parsed.entries.len(), 2);
        assert!(parsed.errors.is_empty());
        assert_eq!(parsed.entries[0].name, "GitHub");
        assert_eq!(parsed.entries[0].username, "dev@example.com");
        assert_eq!(parsed.entries[1].name, "Example");
        assert_eq!(parsed.entries[1].username, "user@example.com");
    }

    #[test]
    fn derives_service_from_url() {
        assert_eq!(
            service_from_url("https://www.example.com/path"),
            "example.com"
        );
        assert_eq!(service_from_url("example.org/login"), "example.org");
        assert_eq!(
            service_from_url("https://user@example.net:8443"),
            "example.net"
        );
        assert_eq!(service_from_url(""), "");
    }

    #[test]
    fn imports_totp_only_entry_into_wallet() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let folder = std::env::temp_dir().join(format!(
            "shadowkey-totp-test-{}-{unique}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&folder);

        let totp = normalize_totp("JBSWY3DPEHPK3PXP", "GitHub").unwrap();
        let mut wallet = Wallet::create(&folder, "senha-mestra-123", "pt").unwrap();
        let item_id = add_entry_to_wallet(&mut wallet, "GitHub", "dev", "", "", &totp).unwrap();

        let entries = entries_summary(&mut wallet).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].has_totp);
        assert!(!entries[0].has_password);

        let fields = wallet.get_fields_by_item(&item_id).unwrap();
        assert!(!fields.iter().any(|field| field.field_type == "PASS"));
        let stored = fields
            .iter()
            .find(|field| field.field_type == "TOTP")
            .unwrap()
            .value
            .clone();
        assert_eq!(
            totp_from_stored(&stored, "GitHub")
                .unwrap()
                .generate(59)
                .to_string()
                .len(),
            6
        );

        wallet.close();
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn imports_parsed_entries_into_wallet() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let folder = std::env::temp_dir().join(format!(
            "shadowkey-import-test-{}-{unique}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&folder);

        let parsed = parse_password_csv(
            "name,url,username,password,note\n\
             Gmail,https://accounts.google.com,user@gmail.com,segredo-123,Conta pessoal\n",
        )
        .unwrap();
        let mut wallet = Wallet::create(&folder, "senha-mestra-123", "pt").unwrap();

        for entry in parsed.entries {
            add_entry_to_wallet(
                &mut wallet,
                &entry.name,
                &entry.username,
                &entry.password,
                &entry.notes,
                &entry.totp,
            )
            .unwrap();
        }

        let entries = entries_summary(&mut wallet).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "Gmail");
        assert_eq!(entries[0].username, "user@gmail.com");
        assert!(entries[0].has_password);
        wallet.close();
        let _ = std::fs::remove_dir_all(&folder);
    }
}
