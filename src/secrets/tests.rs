use super::*;

fn unlock_file(password: &str, file: VaultFile) -> Result<VaultData, String> {
    let mut key = key(password, &file.salt)?;
    let result = decrypt(&key, file);
    key.zeroize();
    result
}

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
