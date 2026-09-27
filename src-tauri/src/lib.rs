//! Casca Tauri do Spiegel: liga o núcleo (`spiegel-core`) à interface web
//! por comandos e eventos. A lógica fica no núcleo; aqui só há a ligação.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use spiegel_core::adb::server_link::AdbServerLink;
use spiegel_core::{
    DeviceRegistry, RegistryOptions, RegistrySnapshot, Session, SessionEvent, SessionOptions, Settings, VideoPacket,
};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager, State};

/// Evento emitido a cada mudança da lista de Dispositivos ou do estado do adb.
const SNAPSHOT_EVENT: &str = "registry-snapshot";

const ADB_FILE: &str = if cfg!(windows) { "adb.exe" } else { "adb" };

struct AppState {
    registry: Mutex<Option<DeviceRegistry>>,
    settings: Mutex<Settings>,
    settings_path: PathBuf,
    bundled_adb: PathBuf,
    /// O `scrcpy-server` 4.1 embutido.
    bundled_server: PathBuf,
    sessions: Mutex<HashMap<u32, Session>>,
    next_session: AtomicU32,
}

impl AppState {
    fn adb_path(&self) -> PathBuf {
        self.settings.lock().unwrap().resolve_adb(&self.bundled_adb)
    }
}

/// (Re)cria o registro com o adb configurado e repassa as mudanças à interface.
fn start_registry(app: &AppHandle, state: &AppState) {
    let link = Arc::new(AdbServerLink::new(state.adb_path()));
    let registry = tauri::async_runtime::block_on(async { DeviceRegistry::start(link, RegistryOptions::default()) });

    let mut changes = registry.subscribe();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Emite o estado atual primeiro: ao trocar de adb, a interface não pode
        // ficar com a lista do registro anterior até a próxima mudança.
        // Termina sozinho quando o registro é trocado ou descartado.
        loop {
            let snapshot = changes.borrow_and_update().clone();
            if let Err(err) = app.emit(SNAPSHOT_EVENT, snapshot) {
                log::warn!("falha ao emitir {SNAPSHOT_EVENT}: {err}");
            }
            if changes.changed().await.is_err() {
                break;
            }
        }
    });

    // Descarta o registro anterior (e o servidor adb continua rodando).
    *state.registry.lock().unwrap() = Some(registry);
}

#[tauri::command]
fn get_snapshot(state: State<'_, AppState>) -> Option<RegistrySnapshot> {
    state.registry.lock().unwrap().as_ref().map(DeviceRegistry::snapshot)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AdbSettings {
    /// Caminho escolhido pelo usuário, ou `null` para o embutido. É o mesmo
    /// `Settings::adb_path`.
    adb_path: Option<PathBuf>,
    bundled_path: PathBuf,
}

#[tauri::command]
fn get_adb_settings(state: State<'_, AppState>) -> AdbSettings {
    AdbSettings {
        adb_path: state.settings.lock().unwrap().adb_path.clone(),
        bundled_path: state.bundled_adb.clone(),
    }
}

/// Troca o adb usado (`null` volta ao embutido) e recomeça o acompanhamento.
#[tauri::command]
fn set_adb_path(app: AppHandle, state: State<'_, AppState>, path: Option<PathBuf>) -> Result<(), String> {
    let path = path.filter(|path| !path.as_os_str().is_empty());
    {
        let mut settings = state.settings.lock().unwrap();
        settings.adb_path = path;
        settings.save(&state.settings_path).map_err(|err| err.to_string())?;
    }
    start_registry(&app, &state);
    Ok(())
}

/// Decisão do usuário: encerrar o servidor adb atual e iniciar um com o adb
/// configurado. Retorna quando o novo estado já foi emitido.
#[tauri::command]
async fn restart_adb_server(state: State<'_, AppState>) -> Result<(), String> {
    let handle = state.registry.lock().unwrap().as_ref().map(DeviceRegistry::restart_handle);
    if let Some(handle) = handle {
        handle.restart_server().await;
    }
    Ok(())
}

/// Inicia uma Sessão de Tela e devolve o id dela. Os eventos chegam por
/// `on_event`: JSON para os eventos da Sessão e binário para os pacotes de
/// vídeo (veja [`encode_packet`]), na ordem em que o núcleo os emite.
#[tauri::command]
async fn start_session(
    app: AppHandle,
    state: State<'_, AppState>,
    serial: String,
    on_event: Channel<InvokeResponseBody>,
) -> Result<u32, String> {
    let link = Arc::new(AdbServerLink::new(state.adb_path()));
    let (session, mut events) = Session::start(link, serial, SessionOptions::new(state.bundled_server.clone()));
    let id = state.next_session.fetch_add(1, Ordering::Relaxed);
    state.sessions.lock().unwrap().insert(id, session);

    tauri::async_runtime::spawn(async move {
        let forget_session = || app.state::<AppState>().sessions.lock().unwrap().remove(&id);
        while let Some(event) = events.recv().await {
            let ended = matches!(event, SessionEvent::Ended { .. });
            let body = match event {
                SessionEvent::VideoPacket(packet) => InvokeResponseBody::Raw(encode_packet(&packet)),
                other => match serde_json::to_string(&other) {
                    Ok(json) => InvokeResponseBody::Json(json),
                    Err(err) => {
                        log::warn!("evento de Sessão sem JSON: {err}");
                        continue;
                    }
                },
            };
            // Sem ninguém do outro lado (a interface recarregou), a Sessão
            // é descartada, o que a encerra; os eventos seguem até o fim.
            if on_event.send(body).is_err() || ended {
                drop(forget_session());
            }
        }
    });
    Ok(id)
}

/// Para a Sessão e espera a desmontagem. O evento `ended` ainda chega pelo canal.
#[tauri::command]
async fn stop_session(state: State<'_, AppState>, id: u32) -> Result<(), String> {
    let session = state.sessions.lock().unwrap().remove(&id);
    if let Some(session) = session {
        session.stop().await;
    }
    Ok(())
}

/// Um pacote de vídeo em binário: 1 byte de flags (bit 0: config, bit 1:
/// quadro-chave), o PTS em u64 big-endian (0 nos pacotes de config) e o
/// conteúdo codificado, intacto. A interface lê isso em `readPacket`
/// (`src/video/packets.ts`).
fn encode_packet(packet: &VideoPacket) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(9 + packet.data.len());
    bytes.push(u8::from(packet.config) | u8::from(packet.key_frame) << 1);
    bytes.extend(packet.pts.unwrap_or(0).to_be_bytes());
    bytes.extend(&packet.data);
    bytes
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(tauri_plugin_log::Builder::default().level(log::LevelFilter::Info).build())?;
            }

            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let settings = Settings::load(&settings_path).unwrap_or_else(|err| {
                log::warn!("configurações ilegíveis em {}, usando os padrões: {err}", settings_path.display());
                Settings::default()
            });
            let resources = app.path().resource_dir()?;
            let bundled_adb = resources.join("platform-tools").join(ADB_FILE);

            app.manage(AppState {
                registry: Mutex::new(None),
                settings: Mutex::new(settings),
                settings_path,
                bundled_adb,
                bundled_server: resources.join("scrcpy-server"),
                sessions: Mutex::new(HashMap::new()),
                next_session: AtomicU32::new(1),
            });
            start_registry(app.handle(), &app.state::<AppState>());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            get_adb_settings,
            set_adb_path,
            restart_adb_server,
            start_session,
            stop_session
        ])
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o Spiegel");
}
