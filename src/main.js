import { invoke } from "@tauri-apps/api/core";
import { confirm as confirmDialog, open } from "@tauri-apps/plugin-dialog";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";

const LAST_FOLDER_KEY = "shadowkey.lastFolder";
const AUTO_LOCK_MS = 5 * 60 * 1000;
const CLIPBOARD_CLEAR_MS = 30 * 1000;

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
};

const lockScreen = document.getElementById("lock-screen");
const vaultScreen = document.getElementById("vault-screen");
const vaultPathEl = document.getElementById("vault-path");
const vaultHintEl = document.getElementById("vault-hint");
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
const detailNotesEl = document.getElementById("detail-notes");
const detailMetaEl = document.getElementById("detail-meta");
const revealBtn = document.getElementById("btn-reveal");
const copyPassBtn = document.getElementById("btn-copy-pass");
const copyUserBtn = document.getElementById("btn-copy-user");
const entryModal = document.getElementById("entry-modal");
const entryForm = document.getElementById("entry-form");
const entryIdEl = document.getElementById("entry-id");
const entryServiceEl = document.getElementById("entry-service");
const entryUsernameEl = document.getElementById("entry-username");
const entryPasswordEl = document.getElementById("entry-password");
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
    "O cofre será salvo no armazenamento privado do aplicativo.";
}

function setFolder(path) {
  state.folder = path;
  state.hasVault = false;
  vaultPathEl.textContent = path;
  vaultPathEl.title = path;
  vaultPathEl.classList.add("selected");
  localStorage.setItem(LAST_FOLDER_KEY, path);
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
  detailCardEl.classList.add("hidden");
  detailEmptyEl.classList.remove("hidden");
}

async function selectEntry(id) {
  state.selectedId = id;
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
      const [details, username, password, notes] = await Promise.all([
        invoke("get_entry", { id: entry.id }),
        invoke("get_secret", { id: entry.id, field: "USER" }),
        invoke("get_secret", { id: entry.id, field: "PASS" }),
        invoke("get_secret", { id: entry.id, field: "NOTE" }),
      ]);

      entryIdEl.value = entry.id;
      entryServiceEl.value = details.name;
      entryUsernameEl.value = username || "";
      entryPasswordEl.value = password || "";
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
  const notes = entryNotesEl.value;

  if (!service) {
    showEntryError("Informe o serviço ou site.");
    return;
  }
  if (!password) {
    showEntryError("Informe a senha.");
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
        notes,
      });
      showToast("Entrada atualizada.", "success");
    } else {
      targetId = await invoke("add_entry", {
        service,
        username,
        password,
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
  document.getElementById("btn-edit").addEventListener("click", () => {
    const entry = state.entries.find((item) => item.id === state.selectedId);
    if (entry) openEntryModal(entry);
  });
  document.getElementById("btn-delete").addEventListener("click", deleteSelectedEntry);
  revealBtn.addEventListener("click", togglePasswordReveal);
  copyUserBtn.addEventListener("click", copyUsername);
  copyPassBtn.addEventListener("click", copyPassword);

  entryForm.addEventListener("submit", saveEntry);
  document.getElementById("btn-cancel-entry").addEventListener("click", () => entryModal.close());
  document.getElementById("btn-close-entry").addEventListener("click", () => entryModal.close());
  entryModal.addEventListener("close", () => {
    entryPasswordEl.value = "";
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

  for (const modal of [entryModal, masterModal]) {
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
  refreshLockButtons();

  const lastFolder = state.mobile ? null : localStorage.getItem(LAST_FOLDER_KEY);
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
