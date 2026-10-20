import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';

let vaultPath = null;

// Elementos da interface
const filePathEl = document.getElementById('file-path');
const logEl = document.getElementById('log');

function log(msg) {
    logEl.textContent += `> ${msg}\n`;
    console.log(msg);
}

// Selecionar arquivo
document.getElementById('btn-select').addEventListener('click', async () => {
    try {
        // Abre diálogo de seleção de arquivo (permite criar novo arquivo na primeira vez)
        const selected = await open({
            multiple: false,
            directory: false,
            filters: [{ name: 'ShadowKey Vault', extensions: ['vault', 'db'] }]
        });
        if (selected) {
            vaultPath = selected;
            filePathEl.textContent = vaultPath;
            log(`Arquivo selecionado: ${vaultPath}`);
        }
    } catch (e) {
        log(`Erro na seleção: ${e}`);
    }
});

// Criar cofre
document.getElementById('btn-create').addEventListener('click', async () => {
    const pass = document.getElementById('master-pass').value;
    if (!vaultPath || !pass) { log('Selecione arquivo e digite a senha'); return; }
    try {
        const res = await invoke('create_vault', { path: vaultPath, masterPassword: pass });
        log(res);
    } catch (e) {
        log(`Erro ao criar: ${e}`);
    }
});

// Desbloquear e listar
document.getElementById('btn-unlock').addEventListener('click', async () => {
    const pass = document.getElementById('master-pass').value;
    if (!vaultPath || !pass) { log('Selecione arquivo e digite a senha'); return; }
    try {
        const entries = await invoke('unlock_and_list', { path: vaultPath, masterPassword: pass });
        const entriesDiv = document.getElementById('entries');
        if (entries.length === 0) {
            entriesDiv.textContent = 'Cofre vazio.';
        } else {
            entriesDiv.innerHTML = entries.map(e => `<div>[${e.id}] ${e.name}</div>`).join('');
        }
        log(`Cofre desbloqueado. ${entries.length} entradas.`);
    } catch (e) {
        log(`Erro ao desbloquear: ${e}`);
    }
});

// Adicionar senha
document.getElementById('btn-add').addEventListener('click', async () => {
    const pass = document.getElementById('master-pass').value;
    const service = document.getElementById('service').value;
    const user = document.getElementById('username').value;
    const pwd = document.getElementById('password').value;
    if (!vaultPath || !pass || !service || !user || !pwd) { log('Preencha todos os campos'); return; }
    try {
        await invoke('add_credential', {
            path: vaultPath,
            masterPassword: pass,
            service,
            username: user,
            password: pwd
        });
        log(`Senha adicionada para ${service}`);
        // Limpar campos
        document.getElementById('service').value = '';
        document.getElementById('username').value = '';
        document.getElementById('password').value = '';
    } catch (e) {
        log(`Erro ao adicionar: ${e}`);
    }
});