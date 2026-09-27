//! Casca Tauri do Spiegel: liga o núcleo (`spiegel-core`) à interface web
//! por comandos e eventos. A lógica fica no núcleo; aqui só há a ligação.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use spiegel_core::adb::server_link::AdbServerLink;
use spiegel_core::{DeviceRegistry, RegistryOptions, RegistrySnapshot, Settings};
use tauri::{AppHandle, Emitter, Manager, State};

/// Evento emitido a cada mudança da lista de Dispositivos ou do estado do adb.
const SNAPSHOT_EVENT: &str = "registry-snapshot";

const ADB_FILE: &str = if cfg!(windows) { "adb.exe" } else { "adb" };

struct AppState {
    registry: Mutex<Option<DeviceRegistry>>,
    settings: Mutex<Settings>,
    settings_path: PathBuf,
    bundled_adb: PathBuf,
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
        // Termina sozinho quando o registro é trocado ou descartado.
        while changes.changed().await.is_ok() {
            let snapshot = changes.borrow_and_update().clone();
            if let Err(err) = app.emit(SNAPSHOT_EVENT, snapshot) {
                log::warn!("falha ao emitir {SNAPSHOT_EVENT}: {err}");
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
    /// Caminho escolhido pelo usuário, ou `null` para o embutido.
    custom_path: Option<PathBuf>,
    bundled_path: PathBuf,
}

#[tauri::command]
fn get_adb_settings(state: State<'_, AppState>) -> AdbSettings {
    AdbSettings {
        custom_path: state.settings.lock().unwrap().adb_path.clone(),
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

/// Decisão do usuário: encerrar o servidor adb atual e iniciar o do Spiegel.
#[tauri::command]
async fn restart_adb_server(state: State<'_, AppState>) -> Result<(), String> {
    let handle = state.registry.lock().unwrap().as_ref().map(DeviceRegistry::restart_handle);
    if let Some(handle) = handle {
        handle.restart_server().await;
    }
    Ok(())
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
            let bundled_adb = app.path().resource_dir()?.join("platform-tools").join(ADB_FILE);

            app.manage(AppState {
                registry: Mutex::new(None),
                settings: Mutex::new(settings),
                settings_path,
                bundled_adb,
            });
            start_registry(app.handle(), &app.state::<AppState>());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_snapshot, get_adb_settings, set_adb_path, restart_adb_server])
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o Spiegel");
}
