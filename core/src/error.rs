use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("token do SmartThings não configurado")]
    MissingToken,
    #[error("nenhum ar-condicionado selecionado")]
    MissingDevice,
    #[error("token do SmartThings inválido ou expirado")]
    Unauthorized,
    #[error("SmartThings respondeu {status}: {body}")]
    Api { status: u16, body: String },
    #[error("falha de rede: {0}")]
    Http(#[from] reqwest::Error),
    #[error("falha ao acessar o Keychain: {0}")]
    Keyring(#[from] keyring::Error),
    #[error("falha de E/S: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON inválido: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Tauri(#[from] tauri::Error),
}

/// Commands return errors to the webview as plain messages.
impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
