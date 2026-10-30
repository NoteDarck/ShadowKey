import { invoke } from "@tauri-apps/api/core";
import { confirm as confirmDialog, open } from "@tauri-apps/plugin-dialog";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import jsQR from "jsqr";

const LAST_FOLDER_KEY = "shadowkey.lastFolder";
const AUTO_LOCK_MS = 5 * 60 * 1000;
const CLIPBOARD_CLEAR_MS = 30 * 1000;
const IMPORT_CSV_ACCEPT = ".csv,.txt,text/csv,text/plain";
const IMPORT_IMAGE_ACCEPT = ".png,.jpg,.jpeg,.webp,.gif,.bmp,image/*";
const IMPORT_ALL_ACCEPT = `${IMPORT_CSV_ACCEPT},${IMPORT_IMAGE_ACCEPT}`;

const state = {
  folder: null,
  hasVault: false,
  unlocked: false,
  entries: [],
  selectedId: null,
  selected: null,
  query: "",
  busy: false,
  busyAction: null,
  mobile: false,
  storageOptions: [],
};

const lockScreen = document.getElementById("lock-screen");
const vaultScreen = document.getElementById("vault-screen");
const vaultPathEl = document.getElementById("vault-path");
const vaultHintEl = document.getElementById("vault-hint");
const storageOptionsEl = document.getElementById("storage-options");
const storageOptionListEl = document.getElementById("storage-option-list");
const importFileEl = document.getElementById("import-file");
const importBtn = document.getElementById("btn-import");
const importModal = document.getElementById("import-modal");
const importTextEl = document.getElementById("import-text");
const importDropEl = document.getElementById("import-drop");
const importResultEl = document.getElementById("import-result");
const masterPassEl = document.getElementById("master-pass");
const masterConfirmEl = document.getElementById("master-pass-confirm");
const confirmWrapEl = document.getElementById("confirm-wrap");
const lockErrorEl = document.getElementById("lock-error");
const unlockBtn = document.getElementById("btn-unlock");
const createBtn = document.getElementById("btn-create");
const entryListEl = document.getElementById("entry-list");
const emptyStateEl = document.getElementById("empty-state");
const entryCountEl = document.getElementById("entry-count");
const vaultFolderEl = document.getElementById("vault-folder");
const searchEl = document.getElementById("search");
const detailEmptyEl = document.getElementById("detail-empty");
const detailCardEl = document.getElementById("detail-card");
const detailAvatarEl = document.getElementById("detail-avatar");
const detailNameEl = document.getElementById("detail-name");
const detailUsernameEl = document.getElementById("detail-username");
const detailUserEl = document.getElementById("detail-user");
const detailPassEl = document.getElementById("detail-pass");
const detailTotpGroupEl = document.getElementById("detail-totp-group");
const detailTotpEl = document.getElementById("detail-totp");
const totpCountdownEl = document.getElementById("totp-countdown");
const totpProgressBarEl = document.getElementById("totp-progress-bar");
const detailNotesEl = document.getElementById("detail-notes");
const detailMetaEl = document.getElementById("detail-meta");
const revealBtn = document.getElementById("btn-reveal");
const copyPassBtn = document.getElementById("btn-copy-pass");
const copyTotpBtn = document.getElementById("btn-copy-totp");
const copyUserBtn = document.getElementById("btn-copy-user");
const entryModal = document.getElementById("entry-modal");
const entryForm = document.getElementById("entry-form");
const entryIdEl = document.getElementById("entry-id");
const entryServiceEl = document.getElementById("entry-service");
const entryUsernameEl = document.getElementById("entry-username");
const entryPasswordEl = document.getElementById("entry-password");
const entryTotpEl = document.getElementById("entry-totp");
const entryNotesEl = document.getElementById("entry-notes");
const entryErrorEl = document.getElementById("entry-error");
const entryModalTitle = document.getElementById("modal-title");
const saveEntryBtn = document.getElementById("btn-save-entry");
const masterModal = document.getElementById("master-modal");
const masterForm = document.getElementById("master-form");
const currentMasterEl = document.getElementById("current-master");
const newMasterEl = document.getElementById("new-master");
const confirmNewMasterEl = document.getElementById("confirm-new-master");
const masterErrorEl = document.getElementById("master-error");
const toastEl = document.getElementById("toast");
const genLengthEl = document.getElementById("gen-length");
const genLengthValueEl = document.getElementById("gen-length-value");

let autoLockTimer = null;
let toastTimer = null;
let clipboardTimer = null;
let totpTimer = null;
let totpRefreshing = false;
let lastActivityAt = 0;

function errorMessage(error) {
  if (typeof error === "string") return error;
  if (error && typeof error.message === "string") return error.message;
  return String(error);
}

function formatBytes(bytes) {
  if (!bytes) return "0 KB";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatDate(value) {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleString("pt-BR", { dateStyle: "short", timeStyle: "short" });
}

function initial(value) {
  const char = (value || "").trim().charAt(0);
  return char ? char.toUpperCase() : "?";
}

function showToast(message, type = "info") {
  toastEl.textContent = message;
  toastEl.className = `toast ${type}`;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    toastEl.classList.add("hidden");
  }, 3600);
}

function showLockError(message) {
  lockErrorEl.textContent = message;
  lockErrorEl.classList.remove("hidden");
}

function hideLockError() {
  lockErrorEl.classList.add("hidden");
  lockErrorEl.textContent = "";
}

function showEntryError(message) {
  entryErrorEl.textContent = message;
  entryErrorEl.classList.remove("hidden");
}

function hideEntryError() {
  entryErrorEl.classList.add("hidden");
  entryErrorEl.textContent = "";
}

function showMasterError(message) {
  masterErrorEl.textContent = message;
  masterErrorEl.classList.remove("hidden");
}

function hideMasterError() {
  masterErrorEl.classList.add("hidden");
  masterErrorEl.textContent = "";
}

function refreshLockButtons() {
  unlockBtn.disabled = state.busy || !state.folder || !state.hasVault;
  createBtn.disabled = state.busy || !state.folder || state.hasVault;
  unlockBtn.textContent = state.busy && state.busyAction === "open" ? "Abrindo..." : "Abrir cofre";
  createBtn.textContent = state.busy && state.busyAction === "create" ? "Criando..." : "Criar novo cofre";
}

function setLockBusy(busy, action = null) {
  state.busy = busy;
  state.busyAction = busy ? action : null;
  refreshLockButtons();
}

function showScreen(name) {
  const isVault = name === "vault";
  lockScreen.classList.toggle("hidden", isVault);
  vaultScreen.classList.toggle("hidden", !isVault);
}

function applyMobileUi() {
  if (!state.mobile) return;

  document.getElementById("btn-choose").classList.add("hidden");
  const portableBtn = document.getElementById("btn-portable");
  portableBtn.textContent = "Usar o armazenamento privado do aplicativo";
  portableBtn.classList.remove("hidden");
  document.querySelector(".badge").textContent = "Android";
  vaultHintEl.textContent =
    "Escolha o armazenamento privado do aplicativo ou o cartão SD.";
}

function storageIcon(kind) {
  if (kind === "removable") return "SD";
  if (kind === "external") return "EXT";
  if (kind === "portable") return "APP";
  return "APP";
}

function renderStorageOptions() {
  storageOptionListEl.replaceChildren();

  for (const option of state.storageOptions) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = `storage-option${option.path === state.folder ? " active" : ""}`;
    button.title = option.path;

    const icon = document.createElement("span");
    icon.className = "storage-option-icon";
    icon.textContent = storageIcon(option.kind);

    const text = document.createElement("span");
    text.className = "storage-option-text";

    const label = document.createElement("span");
    label.className = "storage-option-label";
    label.textContent = option.label;

    const path = document.createElement("span");
    path.className = "storage-option-path";
    path.textContent = option.path;

    text.append(label, path);
    button.append(icon, text);
    button.addEventListener("click", async () => {
      hideLockError();
      await setFolder(option.path);
      showToast(`${option.label} selecionado.`, "info");
    });
    storageOptionListEl.append(button);
  }

  storageOptionsEl.classList.toggle("hidden", state.storageOptions.length === 0);
}

async function loadStorageOptions() {
  try {
    state.storageOptions = await invoke("list_storage_options");
  } catch (error) {
    state.storageOptions = [];
    showLockError(errorMessage(error));
  }
  renderStorageOptions();
}

function setFolder(path) {
  state.folder = path;
  state.hasVault = false;
  vaultPathEl.textContent = path;
  vaultPathEl.title = path;
  vaultPathEl.classList.add("selected");
  localStorage.setItem(LAST_FOLDER_KEY, path);
  renderStorageOptions();
  return inspectCurrentFolder();
}

async function inspectCurrentFolder() {
  if (!state.folder) {
    refreshLockButtons();
    return;
  }

  try {
    const info = await invoke("inspect_vault", { path: state.folder });
    state.folder = info.folder;
    state.hasVault = info.hasVault;
    vaultPathEl.textContent = info.folder;
    vaultPathEl.title = info.folder;
    localStorage.setItem(LAST_FOLDER_KEY, info.folder);

    if (info.hasVault) {
      vaultHintEl.textContent = `Cofre encontrado (${formatBytes(info.sizeBytes)}). Digite a senha mestra para abrir.`;
      vaultHintEl.className = "hint ok";
      confirmWrapEl.classList.add("hidden");
    } else {
      vaultHintEl.textContent =
        "Nenhum cofre nesta pasta. Digite uma senha forte e clique em Criar novo cofre.";
      vaultHintEl.className = "hint warn";
      confirmWrapEl.classList.remove("hidden");
    }
  } catch (error) {
    state.hasVault = false;
    vaultHintEl.textContent = errorMessage(error);
    vaultHintEl.className = "hint warn";
  }

  renderStorageOptions();
  refreshLockButtons();
}

async function chooseFolder() {
  try {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Escolha a pasta do cofre no pendrive ou cartão",
    });

    if (typeof selected === "string") {
      hideLockError();
      await setFolder(selected);
    }
  } catch (error) {
    showLockError(errorMessage(error));
  }
}

async function usePortableFolder() {
  try {
    const path = await invoke("suggest_portable_folder");
    hideLockError();
    await setFolder(path);
    showToast(
      state.mobile
        ? "Armazenamento privado do aplicativo selecionado."
        : "Pasta portátil selecionada. Crie o cofre nela para levar no pendrive.",
      "info",
    );
  } catch (error) {
    showLockError(errorMessage(error));
  }
}

function clearSensitiveInputs() {
  masterPassEl.value = "";
  masterConfirmEl.value = "";
  masterPassEl.type = "password";
  document.getElementById("toggle-master").textContent = "Mostrar";
}

function enterVault(info) {
  state.unlocked = true;
  state.folder = info.folder || state.folder;
  state.hasVault = true;
  state.entries = info.entries || [];
  state.selectedId = null;
  state.selected = null;
  state.query = "";
  searchEl.value = "";
  vaultFolderEl.textContent = state.folder || "";
  vaultFolderEl.title = state.folder || "";
  showDetailEmpty();
  showScreen("vault");
  renderEntries();
  resetAutoLock();
}

async function openVault() {
  if (state.busy) return;
  if (!state.folder) {
    showLockError("Escolha a pasta do cofre.");
    return;
  }
  const password = masterPassEl.value;
  if (!password) {
    showLockError("Digite a senha mestra.");
    return;
  }

  hideLockError();
  setLockBusy(true, "open");

  try {
    const info = await invoke("open_vault", {
      path: state.folder,
      masterPassword: password,
    });
    clearSensitiveInputs();
    enterVault(info);
    showToast("Cofre aberto.", "success");
  } catch (error) {
    showLockError(errorMessage(error));
  } finally {
    setLockBusy(false);
  }
}

async function createVault() {
  if (state.busy) return;
  if (!state.folder) {
    showLockError("Escolha a pasta do cofre.");
    return;
  }

  const password = masterPassEl.value;
  const confirmation = masterConfirmEl.value;

  if (password.length < 8) {
    showLockError("A senha mestra precisa ter pelo menos 8 caracteres.");
    return;
  }
  if (password !== confirmation) {
    showLockError("As senhas não coincidem.");
    return;
  }

  hideLockError();
  setLockBusy(true, "create");

  try {
    const info = await invoke("create_vault", {
      path: state.folder,
      masterPassword: password,
    });
    clearSensitiveInputs();
    enterVault(info);
    showToast("Cofre criado e desbloqueado.", "success");
  } catch (error) {
    showLockError(errorMessage(error));
  } finally {
    setLockBusy(false);
  }
}

async function lockVault(automatic = false) {
  clearAutoLock();
  try {
    await invoke("lock_vault");
  } catch {
    // O processo pode estar encerrando; o cofre é fechado de qualquer forma.
  }

  state.unlocked = false;
  state.entries = [];
  state.selectedId = null;
  state.selected = null;
  state.query = "";
  searchEl.value = "";
  entryListEl.replaceChildren();
  entryCountEl.textContent = "0 entradas";
  detailCardEl.classList.add("hidden");
  detailEmptyEl.classList.remove("hidden");
  detailUserEl.value = "";
  detailPassEl.value = "";
  detailNotesEl.value = "";
  detailNameEl.textContent = "";
  detailUsernameEl.textContent = "";
  detailTotpGroupEl.classList.add("hidden");
  detailTotpEl.value = "";
  totpCountdownEl.textContent = "";
  totpProgressBarEl.style.width = "100%";
  stopTotpTimer();
  if (entryModal.open) entryModal.close();
  if (masterModal.open) masterModal.close();
  clearSensitiveInputs();
  hideLockError();
  showScreen("lock");
  await inspectCurrentFolder();

  showToast(
    automatic
      ? "Cofre bloqueado automaticamente por inatividade."
      : "Cofre bloqueado.",
    "success",
  );
}

function resetAutoLock() {
  if (!state.unlocked) return;
  clearTimeout(autoLockTimer);
  autoLockTimer = setTimeout(() => {
    if (state.unlocked) lockVault(true);
  }, AUTO_LOCK_MS);
}

function clearAutoLock() {
  clearTimeout(autoLockTimer);
  autoLockTimer = null;
}

function noteActivity() {
  if (!state.unlocked) return;
  const now = Date.now();
  if (now - lastActivityAt < 1000) return;
  lastActivityAt = now;
  resetAutoLock();
}

function renderEntries() {
  const query = state.query.trim().toLowerCase();
  const filtered = state.entries.filter((entry) => {
    if (!query) return true;
    return (
      entry.name.toLowerCase().includes(query) ||
      (entry.username || "").toLowerCase().includes(query)
    );
  });

  entryListEl.replaceChildren();

  for (const entry of filtered) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = `entry-item${entry.id === state.selectedId ? " active" : ""}`;
    button.setAttribute("role", "listitem");
    button.addEventListener("click", () => selectEntry(entry.id));

    const avatar = document.createElement("div");
    avatar.className = "avatar";
    avatar.textContent = initial(entry.name);

    const info = document.createElement("div");
    info.className = "entry-info";

    const name = document.createElement("span");
    name.className = "entry-name";
    name.textContent = entry.name;

    const username = document.createElement("span");
    username.className = "entry-user";
    username.textContent = entry.username || "sem usuário";
    if (entry.hasTotp) {
      const badge = document.createElement("span");
      badge.className = "entry-badge";
      badge.textContent = "2FA";
      username.append(badge);
    }

    info.append(name, username);
    button.append(avatar, info);
    entryListEl.append(button);
  }

  entryListEl.classList.toggle("hidden", filtered.length === 0);
  emptyStateEl.classList.toggle("hidden", filtered.length !== 0);

  const title = emptyStateEl.querySelector("strong");
  const description = emptyStateEl.querySelector("span");
  if (state.entries.length === 0) {
    title.textContent = "Nenhuma senha guardada";
    description.textContent = "Clique em “Nova senha” para guardar sua primeira credencial.";
  } else {
    title.textContent = "Nenhuma senha encontrada";
    description.textContent = "Tente outro termo de busca.";
  }

  const count = state.entries.length;
  entryCountEl.textContent = `${count} ${count === 1 ? "entrada" : "entradas"}`;
}

function showDetailEmpty() {
  stopTotpTimer();
  detailCardEl.classList.add("hidden");
  detailEmptyEl.classList.remove("hidden");
  detailTotpGroupEl.classList.add("hidden");
  detailTotpEl.value = "";
  totpCountdownEl.textContent = "";
}

async function selectEntry(id) {
  stopTotpTimer();
  state.selectedId = id;
  state.selected = null;
  renderEntries();

  try {
    const details = await invoke("get_entry", { id });
    if (state.selectedId !== id) return;

    state.selected = details;
    detailNameEl.textContent = details.name;
    detailUsernameEl.textContent = details.username || "sem usuário";
    detailAvatarEl.textContent = initial(details.name);
    detailUserEl.value = details.username || "";
    detailPassEl.value = "";
    detailPassEl.type = "password";
    revealBtn.textContent = "Mostrar";
    revealBtn.disabled = !details.hasPassword;
    copyPassBtn.disabled = !details.hasPassword;
    copyUserBtn.disabled = !details.username;
    detailNotesEl.value = details.notes || "";
    detailNotesEl.placeholder = details.notes ? "" : "Sem notas";
    detailTotpGroupEl.classList.toggle("hidden", !details.hasTotp);
    detailTotpEl.value = "";
    totpCountdownEl.textContent = "";
    totpProgressBarEl.style.width = "100%";
    totpProgressBarEl.classList.remove("expiring");
    startTotpTimer();

    const summary = state.entries.find((entry) => entry.id === id);
    const updated = formatDate(summary?.updatedAt);
    detailMetaEl.textContent = updated ? `Atualizado em ${updated}` : "";

    detailEmptyEl.classList.add("hidden");
    detailCardEl.classList.remove("hidden");
  } catch (error) {
    showToast(errorMessage(error), "error");
  }
}

async function getSecret(field) {
  if (!state.selectedId) return "";
  return invoke("get_secret", { id: state.selectedId, field });
}

function stopTotpTimer() {
  if (totpTimer) {
    clearInterval(totpTimer);
    totpTimer = null;
  }
  totpRefreshing = false;
}

function formatTotpCode(code) {
  if (code.length === 6) return `${code.slice(0, 3)} ${code.slice(3)}`;
  if (code.length === 8) return `${code.slice(0, 4)} ${code.slice(4)}`;
  return code;
}

async function refreshTotp() {
  const id = state.selectedId;
  if (!state.unlocked || !id || !state.selected?.hasTotp || totpRefreshing) return;

  totpRefreshing = true;
  try {
    const totp = await invoke("get_totp", { id });
    if (state.selectedId !== id || !state.selected?.hasTotp) return;

    detailTotpEl.value = formatTotpCode(totp.code);
    const remaining = Math.max(0, Number(totp.secondsRemaining) || 0);
    const period = Math.max(1, Number(totp.period) || 30);
    totpCountdownEl.textContent = `${remaining}s`;
    const percent = Math.max(0, Math.min(100, (remaining / period) * 100));
    totpProgressBarEl.style.width = `${percent}%`;
    totpProgressBarEl.classList.toggle("expiring", remaining <= 5);
  } catch (error) {
    detailTotpEl.value = "";
    totpCountdownEl.textContent = "indisponível";
    totpProgressBarEl.style.width = "0%";
    stopTotpTimer();
    showToast(errorMessage(error), "error");
  } finally {
    totpRefreshing = false;
  }
}

function startTotpTimer() {
  stopTotpTimer();
  if (!state.selected?.hasTotp) return;
  refreshTotp();
  totpTimer = setInterval(refreshTotp, 1000);
}

async function copyTotp() {
  const code = detailTotpEl.value.replace(/\s/g, "");
  await copyToClipboard(code, "Código 2FA copiado.");
}

async function togglePasswordReveal() {
  if (detailPassEl.type === "text") {
    detailPassEl.type = "password";
    detailPassEl.value = "";
    revealBtn.textContent = "Mostrar";
    return;
  }

  try {
    const password = await getSecret("PASS");
    if (!password) {
      showToast("Esta entrada não tem senha.", "error");
      return;
    }
    detailPassEl.value = password;
    detailPassEl.type = "text";
    revealBtn.textContent = "Ocultar";
  } catch (error) {
    showToast(errorMessage(error), "error");
  }
}

function scheduleClipboardClear(value) {
  clearTimeout(clipboardTimer);
  clipboardTimer = setTimeout(async () => {
    try {
      const current = await readText();
      if (current === value) {
        await writeText("");
      }
    } catch {
      // Sem permissão de leitura: não é possível limpar automaticamente.
    }
  }, CLIPBOARD_CLEAR_MS);
}

async function copyToClipboard(value, message) {
  if (!value) {
    showToast("Não há nada para copiar.", "error");
    return;
  }
  try {
    await writeText(value);
    showToast(message, "success");
    scheduleClipboardClear(value);
  } catch (error) {
    showToast(errorMessage(error), "error");
  }
}

async function copyUsername() {
  await copyToClipboard(detailUserEl.value, "Usuário copiado.");
}

async function copyPassword() {
  try {
    const password = await getSecret("PASS");
    await copyToClipboard(password, "Senha copiada. A área de transferência será limpa em 30 s.");
  } catch (error) {
    showToast(errorMessage(error), "error");
  }
}

async function refreshEntries() {
  try {
    state.entries = await invoke("list_entries");
    renderEntries();
  } catch (error) {
    showToast(errorMessage(error), "error");
  }
}

function openImportModal() {
  if (!state.unlocked) return;
  hideImportResult();
  importTextEl.value = "";
  importModal.showModal();
  importTextEl.focus();
}

function triggerImport(accept = IMPORT_ALL_ACCEPT) {
  if (!state.unlocked) return;
  importFileEl.value = "";
  importFileEl.accept = accept;
  importFileEl.click();
}

function hideImportResult() {
  importResultEl.className = "import-result hidden";
  importResultEl.replaceChildren();
}

function setImportBusy(busy) {
  importModal.querySelectorAll("button").forEach((button) => {
    button.disabled = busy;
  });
  importModal.classList.toggle("busy", busy);
}

function appendImportDetails(title, items, kind) {
  if (!items?.length) return;
  const group = document.createElement("div");
  group.className = `import-result-group ${kind}`;
  const heading = document.createElement("strong");
  heading.textContent = title;
  group.append(heading);

  const list = document.createElement("ul");
  const visible = items.slice(0, 8);
  for (const item of visible) {
    const entry = document.createElement("li");
    entry.textContent = item;
    list.append(entry);
  }
  if (items.length > visible.length) {
    const more = document.createElement("li");
    more.textContent = `... e mais ${items.length - visible.length}.`;
    list.append(more);
  }

  group.append(list);
  importResultEl.append(group);
}

function isImageFile(file) {
  return file.type.startsWith("image/") || /\.(png|jpe?g|webp|gif|bmp)$/i.test(file.name);
}

async function loadImageSource(file) {
  if (typeof createImageBitmap === "function") {
    try {
      const bitmap = await createImageBitmap(file);
      return {
        source: bitmap,
        width: bitmap.width,
        height: bitmap.height,
        cleanup: () => bitmap.close?.(),
      };
    } catch {
      // Tenta o caminho alternativo abaixo.
    }
  }

  const url = URL.createObjectURL(file);
  try {
    const image = await new Promise((resolve, reject) => {
      const element = new Image();
      element.onload = () => resolve(element);
      element.onerror = () => reject(new Error(`Não foi possível abrir ${file.name}.`));
      element.src = url;
    });
    return {
      source: image,
      width: image.naturalWidth,
      height: image.naturalHeight,
      cleanup: () => URL.revokeObjectURL(url),
    };
  } catch (error) {
    URL.revokeObjectURL(url);
    throw error;
  }
}

async function decodeQrFromFile(file) {
  const image = await loadImageSource(file);
  try {
    const canvas = document.createElement("canvas");
    canvas.width = image.width;
    canvas.height = image.height;
    const context = canvas.getContext("2d", { willReadFrequently: true });
    if (!context) throw new Error(`Não foi possível ler ${file.name}.`);

    context.drawImage(image.source, 0, 0);
    const pixels = context.getImageData(0, 0, canvas.width, canvas.height);
    const result = jsQR(pixels.data, pixels.width, pixels.height, {
      inversionAttempts: "attemptBoth",
    });
    if (!result?.data) {
      throw new Error(`Nenhum QR code encontrado em ${file.name}.`);
    }
    return result.data;
  } finally {
    image.cleanup();
  }
}

function showImportResult(result) {
  const imported = Number(result.imported) || 0;
  const skipped = Number(result.skipped) || 0;
  const errors = result.errors || [];
  const warnings = result.warnings || [];

  let message =
    imported > 0
      ? `${imported} ${imported === 1 ? "entrada importada" : "entradas importadas"}`
      : "Nenhuma entrada importada";
  if (skipped > 0) message += `; ${skipped} ignorada(s)`;
  if (warnings.length > 0) message += `; ${warnings.length} aviso(s)`;
  if (errors.length > 0) message += `; ${errors.length} erro(s)`;

  hideImportResult();
  const summary = document.createElement("strong");
  summary.textContent = `${message}.`;
  importResultEl.append(summary);
  appendImportDetails("Avisos", warnings, "warning");
  appendImportDetails("Erros", errors, "error");
  importResultEl.className = `import-result ${imported > 0 ? "success" : "error"}`;

  showToast(
    `${message}.`,
    imported > 0
      ? skipped > 0 || warnings.length > 0 || errors.length > 0
        ? "info"
        : "success"
      : "error",
  );
}

async function importFiles(files) {
  const selected = Array.from(files || []);
  if (selected.length === 0) return;

  setImportBusy(true);
  hideImportResult();

  try {
    let imported = 0;
    let skipped = 0;
    const errors = [];
    const warnings = [];

    for (const file of selected) {
      if (file.size > 10 * 1024 * 1024) {
        warnings.push(`${file.name}: arquivo grande demais (limite de 10 MB).`);
        continue;
      }

      let data;
      try {
        data = isImageFile(file) ? await decodeQrFromFile(file) : await file.text();
      } catch (error) {
        errors.push(errorMessage(error));
        continue;
      }

      const result = await invoke("import_vault_data", { data });
      imported += result.imported;
      skipped += result.skipped;
      errors.push(...(result.errors || []));
      warnings.push(...(result.warnings || []));
    }

    await refreshEntries();
    showImportResult({ imported, skipped, errors, warnings });
  } catch (error) {
    showToast(errorMessage(error), "error");
  } finally {
    setImportBusy(false);
    importFileEl.value = "";
    importFileEl.accept = IMPORT_ALL_ACCEPT;
  }
}

async function importDataText(value) {
  const data = (value || "").trim();
  if (!data) {
    showToast("Cole um link otpauth:// ou o conteúdo exportado do Authenticator.", "error");
    return;
  }

  setImportBusy(true);
  hideImportResult();
  try {
    const result = await invoke("import_vault_data", { data });
    await refreshEntries();
    showImportResult(result);
    if (result.imported > 0) importTextEl.value = "";
  } catch (error) {
    showToast(errorMessage(error), "error");
  } finally {
    setImportBusy(false);
  }
}

async function importFromClipboard() {
  try {
    const text = await readText();
    if (!text?.trim()) {
      throw new Error("A área de transferência está vazia.");
    }
    importTextEl.value = text;
    await importDataText(text);
  } catch (error) {
    showToast(errorMessage(error), "error");
  }
}

async function importPastedImage(event) {
  const items = Array.from(event.clipboardData?.items || []);
  const imageItem = items.find((item) => item.type.startsWith("image/"));
  if (!imageItem) return;
  event.preventDefault();
  const file = imageItem.getAsFile();
  if (file) await importFiles([file]);
}

function resetEntryForm() {
  entryForm.reset();
  entryIdEl.value = "";
  entryPasswordEl.type = "password";
  document.getElementById("btn-toggle-entry-pass").textContent = "Mostrar";
  hideEntryError();
  genLengthValueEl.textContent = genLengthEl.value;
}

async function openEntryModal(entry = null) {
  resetEntryForm();

  if (entry) {
    try {
      const [details, username, password, totp, notes] = await Promise.all([
        invoke("get_entry", { id: entry.id }),
        invoke("get_secret", { id: entry.id, field: "USER" }),
        invoke("get_secret", { id: entry.id, field: "PASS" }),
        invoke("get_secret", { id: entry.id, field: "TOTP" }),
        invoke("get_secret", { id: entry.id, field: "NOTE" }),
      ]);

      entryIdEl.value = entry.id;
      entryServiceEl.value = details.name;
      entryUsernameEl.value = username || "";
      entryPasswordEl.value = password || "";
      entryTotpEl.value = totp || "";
      entryNotesEl.value = notes || "";
      entryModalTitle.textContent = "Editar senha";
    } catch (error) {
      showToast(errorMessage(error), "error");
      return;
    }
  } else {
    entryModalTitle.textContent = "Nova senha";
  }

  entryModal.showModal();
  entryServiceEl.focus();
}

async function saveEntry(event) {
  event.preventDefault();
  hideEntryError();

  const id = entryIdEl.value;
  const service = entryServiceEl.value.trim();
  const username = entryUsernameEl.value;
  const password = entryPasswordEl.value;
  const totp = entryTotpEl.value.trim();
  const notes = entryNotesEl.value;

  if (!service) {
    showEntryError("Informe o serviço ou site.");
    return;
  }
  if (!password && !totp) {
    showEntryError("Informe a senha ou o código 2FA.");
    return;
  }

  saveEntryBtn.disabled = true;
  saveEntryBtn.textContent = "Salvando...";

  try {
    let targetId = id;

    if (id) {
      await invoke("update_entry", {
        id,
        service,
        username,
        password,
        totp,
        notes,
      });
      showToast("Entrada atualizada.", "success");
    } else {
      targetId = await invoke("add_entry", {
        service,
        username,
        password,
        totp,
        notes,
      });
      state.selectedId = targetId;
      showToast("Senha guardada.", "success");
    }

    entryModal.close();
    entryPasswordEl.value = "";
    await refreshEntries();

    if (targetId) {
      await selectEntry(targetId);
    }
  } catch (error) {
    showEntryError(errorMessage(error));
  } finally {
    saveEntryBtn.disabled = false;
    saveEntryBtn.textContent = "Salvar";
  }
}

async function deleteSelectedEntry() {
  if (!state.selectedId) return;

  const entry = state.entries.find((item) => item.id === state.selectedId);
  const confirmed = await confirmDialog(
    `Excluir “${entry?.name || "esta senha"}”? Esta ação não pode ser desfeita.`,
    {
      title: "Excluir senha",
      kind: "warning",
      okLabel: "Excluir",
      cancelLabel: "Cancelar",
    },
  );

  if (!confirmed) return;

  try {
    await invoke("delete_entry", { id: state.selectedId });
    state.selectedId = null;
    state.selected = null;
    showDetailEmpty();
    await refreshEntries();
    showToast("Entrada excluída.", "success");
  } catch (error) {
    showToast(errorMessage(error), "error");
  }
}

function openMasterModal() {
  masterForm.reset();
  hideMasterError();
  masterModal.showModal();
  currentMasterEl.focus();
}

async function changeMasterPassword(event) {
  event.preventDefault();
  hideMasterError();

  const currentPassword = currentMasterEl.value;
  const newPassword = newMasterEl.value;
  const confirmation = confirmNewMasterEl.value;

  if (!currentPassword) {
    showMasterError("Digite a senha atual.");
    return;
  }
  if (newPassword.length < 8) {
    showMasterError("A nova senha precisa ter pelo menos 8 caracteres.");
    return;
  }
  if (newPassword !== confirmation) {
    showMasterError("As novas senhas não coincidem.");
    return;
  }

  try {
    await invoke("change_master_password", {
      currentPassword,
      newPassword,
    });
    masterModal.close();
    masterForm.reset();
    showToast("Senha mestra alterada.", "success");
  } catch (error) {
    showMasterError(errorMessage(error));
  }
}

async function generatePassword() {
  try {
    const password = await invoke("generate_password", {
      length: Number(genLengthEl.value),
      lowercase: document.getElementById("gen-lower").checked,
      uppercase: document.getElementById("gen-upper").checked,
      digits: document.getElementById("gen-digits").checked,
      special: document.getElementById("gen-special").checked,
      avoidAmbiguous: document.getElementById("gen-ambiguous").checked,
    });

    entryPasswordEl.value = password;
    entryPasswordEl.type = "text";
    document.getElementById("btn-toggle-entry-pass").textContent = "Ocultar";
    entryPasswordEl.focus();
    entryPasswordEl.setSelectionRange(password.length, password.length);
  } catch (error) {
    showToast(errorMessage(error), "error");
  }
}

function bindEvents() {
  document.getElementById("btn-choose").addEventListener("click", chooseFolder);
  document.getElementById("btn-portable").addEventListener("click", usePortableFolder);
  document.getElementById("btn-refresh-storage").addEventListener("click", loadStorageOptions);
  unlockBtn.addEventListener("click", openVault);
  createBtn.addEventListener("click", createVault);
  document.getElementById("btn-lock").addEventListener("click", () => lockVault(false));
  document.getElementById("btn-change-pass").addEventListener("click", openMasterModal);

  document.getElementById("toggle-master").addEventListener("click", () => {
    const show = masterPassEl.type === "password";
    masterPassEl.type = show ? "text" : "password";
    document.getElementById("toggle-master").textContent = show ? "Ocultar" : "Mostrar";
  });

  masterPassEl.addEventListener("keydown", (event) => {
    if (event.key === "Enter") openVault();
  });
  masterConfirmEl.addEventListener("keydown", (event) => {
    if (event.key === "Enter") createVault();
  });

  searchEl.addEventListener("input", () => {
    state.query = searchEl.value;
    renderEntries();
  });

  document.getElementById("btn-new-entry").addEventListener("click", () => openEntryModal());
  importBtn.addEventListener("click", openImportModal);
  document.getElementById("btn-close-import").addEventListener("click", () => importModal.close());
  document.getElementById("btn-import-csv").addEventListener("click", () =>
    triggerImport(IMPORT_CSV_ACCEPT),
  );
  document.getElementById("btn-import-qr").addEventListener("click", () =>
    triggerImport(IMPORT_IMAGE_ACCEPT),
  );
  document.getElementById("btn-import-text").addEventListener("click", () =>
    importDataText(importTextEl.value),
  );
  document.getElementById("btn-import-clipboard").addEventListener("click", importFromClipboard);
  importFileEl.addEventListener("change", () => importFiles(importFileEl.files));
  importDropEl.addEventListener("click", () => triggerImport(IMPORT_ALL_ACCEPT));
  importDropEl.addEventListener("keydown", (event) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      triggerImport(IMPORT_ALL_ACCEPT);
    }
  });
  for (const eventName of ["dragenter", "dragover"]) {
    importDropEl.addEventListener(eventName, (event) => {
      event.preventDefault();
      importDropEl.classList.add("dragging");
    });
  }
  for (const eventName of ["dragleave", "drop"]) {
    importDropEl.addEventListener(eventName, (event) => {
      event.preventDefault();
      importDropEl.classList.remove("dragging");
    });
  }
  importDropEl.addEventListener("drop", (event) => importFiles(event.dataTransfer?.files));
  importModal.addEventListener("paste", importPastedImage);
  document.getElementById("btn-edit").addEventListener("click", () => {
    const entry = state.entries.find((item) => item.id === state.selectedId);
    if (entry) openEntryModal(entry);
  });
  document.getElementById("btn-delete").addEventListener("click", deleteSelectedEntry);
  revealBtn.addEventListener("click", togglePasswordReveal);
  copyUserBtn.addEventListener("click", copyUsername);
  copyPassBtn.addEventListener("click", copyPassword);
  copyTotpBtn.addEventListener("click", copyTotp);

  entryForm.addEventListener("submit", saveEntry);
  document.getElementById("btn-cancel-entry").addEventListener("click", () => entryModal.close());
  document.getElementById("btn-close-entry").addEventListener("click", () => entryModal.close());
  entryModal.addEventListener("close", () => {
    entryPasswordEl.value = "";
    entryTotpEl.value = "";
    hideEntryError();
  });

  document.getElementById("btn-toggle-entry-pass").addEventListener("click", () => {
    const show = entryPasswordEl.type === "password";
    entryPasswordEl.type = show ? "text" : "password";
    document.getElementById("btn-toggle-entry-pass").textContent = show ? "Ocultar" : "Mostrar";
  });

  genLengthEl.addEventListener("input", () => {
    genLengthValueEl.textContent = genLengthEl.value;
  });
  document.getElementById("btn-generate").addEventListener("click", generatePassword);

  masterForm.addEventListener("submit", changeMasterPassword);
  document.getElementById("btn-cancel-master").addEventListener("click", () => masterModal.close());
  document.getElementById("btn-close-master").addEventListener("click", () => masterModal.close());
  masterModal.addEventListener("close", () => {
    masterForm.reset();
    hideMasterError();
  });

  importModal.addEventListener("close", () => {
    importTextEl.value = "";
    importFileEl.accept = IMPORT_ALL_ACCEPT;
    hideImportResult();
  });

  for (const modal of [entryModal, masterModal, importModal]) {
    modal.addEventListener("click", (event) => {
      if (event.target === modal) modal.close();
    });
  }

  for (const eventName of ["pointerdown", "keydown", "wheel", "touchstart"]) {
    document.addEventListener(eventName, noteActivity, { passive: true });
  }

  document.addEventListener("keydown", (event) => {
    if (!state.unlocked) return;
    const modifier = event.ctrlKey || event.metaKey;
    if (modifier && event.key.toLowerCase() === "k") {
      event.preventDefault();
      searchEl.focus();
    }
    if (modifier && event.key.toLowerCase() === "l") {
      event.preventDefault();
      lockVault(false);
    }
  });
}

async function init() {
  bindEvents();

  try {
    state.mobile = await invoke("is_mobile");
  } catch {
    state.mobile = false;
  }
  applyMobileUi();
  await loadStorageOptions();
  refreshLockButtons();

  const lastFolder = localStorage.getItem(LAST_FOLDER_KEY);
  if (lastFolder) {
    await setFolder(lastFolder);
  } else if (state.mobile) {
    await usePortableFolder();
  }

  try {
    const status = await invoke("get_status");
    if (status.unlocked) {
      enterVault(status);
    }
  } catch {
    // Primeira execução ou backend indisponível: permanece na tela de abertura.
  }
}

init();
