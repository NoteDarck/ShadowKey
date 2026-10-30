# ShadowKey

Cofre de senhas portátil para pendrives, cartões SD e armazenamento externo. O cofre é local, criptografado e não depende de nuvem.

**Versão atual:** `0.1.2`

[Baixar a última versão](https://github.com/NoteDarck/ShadowKey/releases/latest)

## Recursos

- Cofre local com criptografia autenticada **XChaCha20-Poly1305** e derivação de chave **Argon2id**.
- Armazenamento em pasta comum, pendrive, cartão SD ou armazenamento externo no Android.
- Modo pendrive: usa a pasta `ShadowKeyVault` ao lado do aplicativo.
- Cadastro, edição, busca, cópia e exclusão de credenciais.
- Gerador de senhas fortes.
- Suporte a códigos **TOTP/2FA**, com contagem regressiva e cópia rápida.
- Entradas somente com 2FA, sem senha.
- Importação de senhas em **CSV** do Google Senhas, Chrome, Bitwarden e 1Password.
- Importação de 2FA do **Google Authenticator** por imagem de QR code, `otpauth-migration://` ou `otpauth://`.
- Importação por arrastar e soltar, seleção de arquivos ou colagem de texto.
- Bloqueio automático após 5 minutos de inatividade.
- Limpeza automática da área de transferência após 30 segundos.
- Aplicativos para Linux, Windows, macOS e Android.

## Segurança

- O cofre fica somente no dispositivo escolhido. Nada é enviado para a internet.
- A senha mestra não é armazenada. Sem ela, os dados não podem ser recuperados.
- O arquivo do cofre é o `nswallet.dat` dentro da pasta selecionada.
- O CSV exportado do Google Senhas e os QR codes do Google Authenticator contêm segredos em texto puro.
- Apague CSVs, prints e exports depois da importação.
- Mantenha backups do cofre. Um pendrive pode ser perdido, corrompido ou roubado.

## Download e instalação

Baixe os arquivos na página de [releases](https://github.com/NoteDarck/ShadowKey/releases/latest).

- **Windows:** `.msi` ou `.exe`
- **macOS:** `.dmg`
- **Linux:** `.AppImage`, `.deb` ou `.rpm`
- **Android:** `.apk`

O APK Android é assinado com a chave de release do ShadowKey. Atualizações futuras usam a mesma chave.

## Como usar

1. Abra o ShadowKey.
2. Escolha a pasta do cofre ou use **Usar a pasta ao lado do aplicativo**.
3. Crie um cofre novo com uma senha mestra forte ou abra um cofre existente.
4. Adicione uma senha em **Nova senha**.
5. Use **Importar senhas / 2FA** para trazer dados de outros gerenciadores.
6. Clique em **Bloquear** ao terminar.

## Importar senhas do Google Senhas

O Google permite exportar as senhas em CSV. O ShadowKey importa esse arquivo localmente.

1. Acesse [passwords.google.com](https://passwords.google.com).
2. Abra **Configurações** e escolha **Exportar senhas**.
3. Confirme e baixe o arquivo CSV.
4. No ShadowKey, abra **Importar senhas / 2FA**.
5. Clique em **Escolher CSV** e selecione o arquivo baixado.
6. Confira as entradas importadas e apague o CSV.

Se o Google entregar um arquivo `.zip`, extraia o `Passwords.csv` antes de importar.

O CSV do Google tem colunas como `name`, `url`, `username`, `password` e `note`. O ShadowKey também aceita variações de CSV do Chrome, Bitwarden e 1Password.

## Importar 2FA do Google Authenticator

O Google Authenticator exporta as contas por QR code. O ShadowKey lê o QR code de uma imagem e converte as contas em entradas TOTP.

1. Abra o Google Authenticator no celular.
2. Toque em **Transferir contas** e depois em **Exportar contas**.
3. Selecione as contas que deseja exportar.
4. Tire um print de cada QR code exibido.
5. No ShadowKey, abra **Importar senhas / 2FA**.
6. Clique em **Escolher QR** e selecione um ou mais prints.
7. Confira as contas importadas e apague os prints.

Também é possível:

- arrastar as imagens para a área de importação;
- colar um print com `Ctrl+V`;
- colar o link `otpauth-migration://offline?data=...` no campo de texto;
- colar links `otpauth://totp/...` individualmente.

Contas **HOTP** são ignoradas; apenas TOTP é suportado. O QR code de exportação do Google Authenticator contém todos os segredos selecionados. Não compartilhe esse QR code.

## Importar CSV de outros gerenciadores

Formatos reconhecidos:

- Google Password Manager / Chrome
- Bitwarden
- 1Password
- CSVs com colunas em português, como `nome`, `usuário`, `senha`, `notas` e `totp`

O arquivo precisa ter uma coluna de senha ou de 2FA e uma coluna de nome ou URL. Separadores `,`, `;` e tab são detectados automaticamente.

## Onde ficam os dados

- O cofre é salvo como `nswallet.dat` na pasta escolhida.
- No modo pendrive, a pasta padrão é `ShadowKeyVault`.
- No Android, o app também lista armazenamentos externos disponíveis.
- Não há sincronização em nuvem, conta online ou telemetria.

## Desenvolvimento

Pré-requisitos:

- Node.js 20 ou superior
- Rust stable
- Dependências do Tauri 2 para a plataforma desejada
- Android SDK/NDK para compilar o APK

Instale as dependências:

```bash
npm install
```

Execute em modo de desenvolvimento:

```bash
npm run tauri dev
```

Gere o build de produção:

```bash
npm run tauri build
```

Verificações:

```bash
npm run build
node --check src/main.js

cd src-tauri
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Build Android:

```bash
npm run tauri -- android init
npm run tauri -- android build --apk
```

## Release

O workflow em `.github/workflows/release.yml` gera os bundles desktop e o APK Android.

Para publicar uma nova versão:

1. Atualize a versão em `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` e `src-tauri/Cargo.lock`.
2. Rode os testes e o build.
3. Faça commit e push.
4. Execute o workflow **Release** via `workflow_dispatch` ou faça push para a branch `release`.
5. Se os secrets de assinatura Android não estiverem configurados, use `allow_debug_signing=true` somente para testes e re-assine o APK com a chave de release antes de distribuir.

Secrets necessários para assinatura Android:

- `ANDROID_KEY_BASE64`
- `ANDROID_KEY_ALIAS`
- `ANDROID_KEY_PASSWORD`

Nunca faça commit da keystore, senhas, CSVs de senhas ou QR codes de exportação.

## Estrutura do projeto

```text
.
├── src/                  # Frontend (HTML, CSS, JavaScript)
├── src-tauri/            # Backend Rust e configuração do Tauri
├── .github/workflows/    # CI de release
├── package.json
└── README.md
```

## Limitações

- Não há sincronização em nuvem.
- Não há extensão de navegador ou preenchimento automático.
- Não há desbloqueio por biometria.
- A importação de QR code é feita por imagem/arquivo, não pela câmera.
- Contas HOTP não são suportadas.
- Não há recuperação de senha mestra.

## Licença

ShadowKey é distribuído sob a **GNU General Public License v2.0**. Consulte [LICENSE](LICENSE).
