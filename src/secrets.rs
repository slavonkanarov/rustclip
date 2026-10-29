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
        let name = "Импортированные";
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

#[cfg(test)]
fn unlock_file(password: &str, file: VaultFile) -> Result<VaultData, String> {
    let mut key = key(password, &file.salt)?;
    let result = decrypt(&key, file);
    key.zeroize();
    result
}

fn decrypt(key: &[u8; 32], file: VaultFile) -> Result<VaultData, String> {
    let plaintext = ChaCha20Poly1305::new(key.into())
        .decrypt(Nonce::from_slice(&file.nonce), file.ciphertext.as_ref());
    let mut plaintext =
        plaintext.map_err(|_| "Неверный PIN/пароль или повреждено хранилище".to_string())?;
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
mod tests {
    use super::*;

    #[test]
    fn password_derives_distinct_key() {
        let salt = [7; 16];
        assert_ne!(key("1234", &salt).unwrap(), key("1235", &salt).unwrap());
        assert_ne!(key("1234", &salt).unwrap(), key("1234", &[8; 16]).unwrap());
    }
    #[test]
    fn session_saves_existing_vault_without_password() {
        let vault = VaultData {
            secrets: vec![Secret {
                name: "Пример".into(),
                value: "значение".into(),
                app: String::new(),
                folder: String::new(),
                hidden: true,
            }],
            folders: Vec::new(),
        };
        let original = encrypt("1234", true, &vault).unwrap();
        let session = SessionKey {
            key: key("1234", &original.salt).unwrap(),
            salt: original.salt,
        };
        assert_eq!(
            decrypt(&session.key, original).unwrap()[0].value,
            "значение"
        );
        let updated = encrypt_session(&session, true, &vault).unwrap();
        assert_eq!(updated.salt, session.salt);
        assert_eq!(decrypt(&session.key, updated).unwrap()[0].value, "значение");
    }
    #[test]
    fn vault_contains_no_plaintext_and_requires_password() {
        let secret = Secret {
            name: "Банк".into(),
            value: "тестовый-секрет-123".into(),
            app: "Bank.exe".into(),
            folder: String::new(),
            hidden: true,
        };
        let file = encrypt(
            "1234",
            true,
            &VaultData {
                secrets: vec![secret],
                folders: Vec::new(),
            },
        )
        .unwrap();
        let bytes = serde_json::to_vec(&file).unwrap();
        let stored = String::from_utf8(bytes.clone()).unwrap();
        assert!(!stored.contains("Банк"));
        assert!(!stored.contains("тестовый-секрет-123"));
        assert!(!stored.contains("Bank.exe"));
        let restored: VaultFile = serde_json::from_slice(&bytes).unwrap();
        assert!(unlock_file("wrong", restored).is_err());
        assert_eq!(
            unlock_file("1234", file).unwrap()[0].value,
            "тестовый-секрет-123"
        );
    }
    #[test]
    fn folder_app_is_inherited_unless_secret_overrides_it() {
        let mut vault = VaultData {
            secrets: vec![Secret {
                name: "Вход".into(),
                value: "x".into(),
                app: String::new(),
                folder: "Работа".into(),
                hidden: true,
            }],
            folders: vec![Folder {
                name: "Работа".into(),
                app: "Code.exe".into(),
            }],
        };
        assert_eq!(vault.app_for(&vault[0]), "Code.exe");
        vault[0].app = "Firefox.exe".into();
        assert_eq!(vault.app_for(&vault[0]), "Firefox.exe");
        assert!(vault.name_in_folder("вход", "Работа", None));
        assert!(!vault.name_in_folder("Вход", "Личное", None));
        vault.folders.push(Folder {
            name: "Личное".into(),
            app: String::new(),
        });
        vault.secrets.push(Secret {
            name: "Вход".into(),
            value: "other".into(),
            app: String::new(),
            folder: "Личное".into(),
            hidden: true,
        });
        assert!(vault.name_in_folder("вход", "Личное", None));
        assert!(!vault.name_in_folder("Вход", "Личное", Some(1)));
    }
    #[test]
    fn old_secret_list_opens_without_folders() {
        let old = serde_json::json!([{"name":"Вход","value":"x","app":"Code.exe"}]);
        let salt = [3; 16];
        let nonce = [4; 12];
        let key = key("1234", &salt).unwrap();
        let ciphertext = ChaCha20Poly1305::new(&key.into())
            .encrypt(Nonce::from_slice(&nonce), old.to_string().as_bytes())
            .unwrap();
        let mut vault = unlock_file(
            "1234",
            VaultFile {
                pin: true,
                salt,
                nonce,
                ciphertext,
            },
        )
        .unwrap();
        assert_eq!(vault[0].name, "Вход");
        assert!(vault[0].hidden);
        assert!(vault.folders.is_empty());
        assert!(vault.place_unfiled());
        assert_eq!(vault[0].folder, "Импортированные");
        assert_eq!(vault.folders.len(), 1);
    }
}
