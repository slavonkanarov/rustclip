use super::*;
#[test]
fn unicode_payload_is_terminated_and_roundtrips() {
    let c = text_clip("Привет 🦀".into());
    let b = &c.formats[0].1;
    assert_eq!(&b[b.len() - 2..], &[0, 0]);
    let w: Vec<_> = b[..b.len() - 2]
        .chunks_exact(2)
        .map(|p| u16::from_le_bytes([p[0], p[1]]))
        .collect();
    assert_eq!(String::from_utf16(&w).unwrap(), c.text);
}
#[test]
fn exclusions_match_whole_executable() {
    assert!(excluded("C:\\Games\\PioneerGame.exe", "pioneergame.exe"));
    assert!(excluded("C:\\Users\\me\\Code.exe", "code"));
    assert!(!excluded("C:\\Users\\me\\CodeHelper.exe", "code"));
    assert!(!excluded(
        "C:\\Games\\NotPioneerGame.exe",
        "PioneerGame.exe"
    ));
    assert!(!excluded("C:\\Games\\PioneerGame.exe", ""));
}
#[test]
fn malformed_dropfiles_is_rejected() {
    assert!(file_list(&[0; 10]).is_none());
    let mut b = vec![0; 20];
    b[..4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(file_list(&b).is_none());
}
#[test]
fn dropfiles_preserves_multiple_paths() {
    let mut b = vec![0; 20];
    b[..4].copy_from_slice(&20u32.to_le_bytes());
    b[16..20].copy_from_slice(&1u32.to_le_bytes());
    b.extend(
        "C:\\one.txt\0D:\\два.png\0\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes),
    );
    assert_eq!(file_list(&b).unwrap(), "C:\\one.txt\nD:\\два.png");
}
