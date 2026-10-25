#[cfg(unix)]
#[test]
fn backend_paths_reject_non_utf8_and_resolve_missing_descendants_of_symlinks() {
    use std::os::unix::ffi::OsStringExt;
    let temp = tempfile::tempdir().unwrap();
    let invalid = temp.path().join(std::ffi::OsString::from_vec(vec![0xff]));
    let error = crate::backend::identity(Some(&invalid), None).unwrap_err();
    assert_eq!(error.exit_code, 2);
    assert_eq!(error.message, "database path must be valid UTF-8");
    assert!(!invalid.exists());
    let real = temp.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let path = link.join("missing").join("tt.db");
    let before = crate::backend::identity(Some(&path), None).unwrap();
    std::fs::create_dir(real.join("missing")).unwrap();
    let after = crate::backend::identity(Some(&path), None).unwrap();
    assert_eq!(before, after);
    assert_eq!(
        before,
        format!(
            "local:{}",
            real.canonicalize().unwrap().join("missing/tt.db").display()
        )
    );
}
