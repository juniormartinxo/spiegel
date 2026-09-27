//! Configurações do Spiegel guardadas em disco.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Caminho de um adb escolhido pelo usuário. `None` usa o adb embutido.
    pub adb_path: Option<PathBuf>,
}

impl Settings {
    /// Lê as configurações. Arquivo inexistente dá os valores padrão.
    pub fn load(path: &Path) -> std::io::Result<Self> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|err| std::io::Error::new(ErrorKind::InvalidData, err)),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err),
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec_pretty(self).map_err(|err| std::io::Error::new(ErrorKind::InvalidData, err))?;
        std::fs::write(path, json)
    }

    /// O adb a usar: o escolhido pelo usuário ou o embutido.
    pub fn resolve_adb(&self, bundled: &Path) -> PathBuf {
        self.adb_path.clone().unwrap_or_else(|| bundled.to_path_buf())
    }
}
