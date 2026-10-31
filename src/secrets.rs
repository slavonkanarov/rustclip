use argon2::Argon2;
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce, aead::Aead};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::model;

#[derive(Clone, Serialize, Deserialize)]
pub struct Secret {
    pub name: String,
    pub value: String,
    pub app: String,
    #[serde(default)]
    pub folder: String,
    #[serde(default = "default_hidden")]
    pub hidden: bool,
}
fn default_hidden() -> bool {
    true
}
impl Secret {
    pub fn clear(&mut self) {
        self.name.zeroize();
        self.value.zeroize();
        self.app.zeroize();
        self.folder.zeroize();
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Folder {
    pub name: String,
    pub app: String,
}

#[derive(Default, Serialize, Deserialize)]
pub struct VaultData {
    pub secrets: Vec<Secret>,
    pub folders: Vec<Folder>,
}

impl VaultData {
    pub fn name_in_folder(&self, name: &str, folder: &str, except: Option<usize>) -> bool {
        self.secrets.iter().enumerate().any(|(i, secret)| {
            Some(i) != except
                && secret.folder == folder
                && secret.name.to_lowercase() == name.to_lowercase()
        })
    }
    pub fn app_for<'a>(&'a self, secret: &'a Secret) -> &'a str {
        if !secret.app.is_empty() {
            &secret.app
        } else {
            self.folders
                .iter()
                .find(|folder| folder.name == secret.folder)
                .map_or("", |folder| folder.app.as_str())
        }
    }
    pub fn import_folder(&mut self) -> String {
        let name = "Imported";
        if !self.folders.iter().any(|folder| folder.name == name) {
            self.folders.push(Folder {
                name: name.into(),
                app: String::new(),
            });
        }
        name.into()
    }
    pub fn place_unfiled(&mut self) -> bool {
        if !self.secrets.iter().any(|secret| {
            !self
                .folders
                .iter()
                .any(|folder| folder.name == secret.folder)
        }) {
            return false;
        }
        let folder = self.import_folder();
        for secret in &mut self.secrets {
            if secret.folder.is_empty()
                || !self.folders.iter().any(|entry| entry.name == secret.folder)
            {
                secret.folder = folder.clone();
            }
        }
        true
    }
}

impl std::ops::Deref for VaultData {
    type Target = Vec<Secret>;
    fn deref(&self) -> &Self::Target {
        &self.secrets
    }
}
impl std::ops::DerefMut for VaultData {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.secrets
    }
}

#[derive(Serialize, Deserialize)]
struct VaultFile {
    pin: bool,
    salt: [u8; 16],
    nonce: [u8; 12],
    ciphertext: Vec<u8>,
}

pub struct SessionKey {
    key: [u8; 32],
    salt: [u8; 16],
}

impl Drop for SessionKey {
    fn drop(&mut self) {
        self.key.zeroize();
        self.salt.zeroize();
    }
}

pub fn exists() -> bool {
    path().exists()
}

fn path() -> std::path::PathBuf {
    model::directory().join("secrets.vault")
}

pub fn mode() -> Result<bool, String> {
    Ok(read_file()?.pin)
}

fn read_file() -> Result<VaultFile, String> {
    serde_json::from_slice(&std::fs::read(path()).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

fn key(password: &str, salt: &[u8; 16]) -> Result<[u8; 32], String> {
    let mut key = [0; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| e.to_string())?;
    Ok(key)
}

pub fn unlock_session(password: &str) -> Result<(VaultData, SessionKey), String> {
    let file = read_file()?;
    let session = SessionKey {
        key: key(password, &file.salt)?,
        salt: file.salt,
    };
    let pin = file.pin;
    let mut vault = decrypt(&session.key, file)?;
    if vault.place_unfiled() {
        save_session(&session, pin, &vault)?;
    }
    Ok((vault, session))
}

fn decrypt(key: &[u8; 32], file: VaultFile) -> Result<VaultData, String> {
    let plaintext = ChaCha20Poly1305::new(key.into())
        .decrypt(Nonce::from_slice(&file.nonce), file.ciphertext.as_ref());
    let mut plaintext =
        plaintext.map_err(|_| "Incorrect PIN/password or damaged vault".to_string())?;
    let result = serde_json::from_slice(&plaintext)
        .or_else(|_| {
            serde_json::from_slice::<Vec<Secret>>(&plaintext).map(|secrets| VaultData {
                secrets,
                folders: Vec::new(),
            })
        })
        .map_err(|e| e.to_string());
    plaintext.zeroize();
    result
}

pub fn save(password: &str, pin: bool, vault: &VaultData) -> Result<(), String> {
    model::write_json(&path(), &encrypt(password, pin, vault)?)
}

pub fn save_session(session: &SessionKey, pin: bool, vault: &VaultData) -> Result<(), String> {
    model::write_json(&path(), &encrypt_session(session, pin, vault)?)
}

fn encrypt_session(
    session: &SessionKey,
    pin: bool,
    vault: &VaultData,
) -> Result<VaultFile, String> {
    let mut nonce = [0; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let mut plaintext = serde_json::to_vec(vault).map_err(|e| e.to_string())?;
    let ciphertext = ChaCha20Poly1305::new((&session.key).into())
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_ref());
    plaintext.zeroize();
    Ok(VaultFile {
        pin,
        salt: session.salt,
        nonce,
        ciphertext: ciphertext.map_err(|e| e.to_string())?,
    })
}

fn encrypt(password: &str, pin: bool, vault: &VaultData) -> Result<VaultFile, String> {
    let mut salt = [0; 16];
    let mut nonce = [0; 12];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let mut key = key(password, &salt)?;
    let mut plaintext = serde_json::to_vec(vault).map_err(|e| e.to_string())?;
    let ciphertext =
        ChaCha20Poly1305::new(&key.into()).encrypt(Nonce::from_slice(&nonce), plaintext.as_ref());
    key.zeroize();
    plaintext.zeroize();
    let ciphertext = ciphertext.map_err(|e| e.to_string())?;
    Ok(VaultFile {
        pin,
        salt,
        nonce,
        ciphertext,
    })
}

#[cfg(test)]
mod tests;
