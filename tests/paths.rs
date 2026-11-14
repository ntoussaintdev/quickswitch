//! T1 — path parsing (plan §9.1).

use std::path::{Path, PathBuf};

use quickswitch::actions::paths::{
    PathCheck, UNC_NOT_LISTED, check, completion, file, list_dirs, looks_like_path, resolve,
};
use quickswitch::search::{Kind, Target, file_item};

fn home() -> PathBuf {
    PathBuf::from(r"C:\Users\alice")
}

fn ok(input: &str) -> PathBuf {
    resolve(input, &home()).expect("path-like").expect("valid")
}

fn err(input: &str) -> String {
    resolve(input, &home())
        .expect("path-like")
        .expect_err("should be invalid")
}

#[test]
fn bare_words_are_not_paths() {
    for s in [
        "projects",
        "Visual Studio",
        "home",
        "/home/alice",
        "",
        "   ",
        ".git",
        "..",
        "c",
        "c:x",
    ] {
        assert!(!looks_like_path(s), "{s:?} should not be path-like");
        assert!(resolve(s, &home()).is_none());
    }
}

#[test]
fn tilde_expands_to_home() {
    assert_eq!(ok("~"), home());
    assert_eq!(ok("~/"), home());
    assert_eq!(ok("~/src"), home().join("src"));
    assert_eq!(ok("~\\src\\app"), home().join("src").join("app"));
    assert_eq!(ok("~/src//app/"), home().join("src").join("app"));
}

#[test]
fn other_users_home_is_not_translated() {
    assert!(err("~bob/docs").contains("~/"));
}

#[test]
fn dot_is_relative_to_home() {
    assert_eq!(ok("./notes"), home().join("notes"));
    assert_eq!(ok(".\\notes"), home().join("notes"));
    assert_eq!(ok("."), home());
    assert_eq!(ok("./a/../b"), home().join("b"));
}

#[test]
fn drive_paths_normalize_separators() {
    assert_eq!(ok("C:/src/app"), PathBuf::from(r"C:\src\app"));
    assert_eq!(ok("c:/src/app/"), PathBuf::from(r"C:\src\app"));
    assert_eq!(ok("D:"), PathBuf::from(r"D:\"));
    assert_eq!(ok("D:/"), PathBuf::from(r"D:\"));
    assert_eq!(ok(r"C:\dev\work"), PathBuf::from(r"C:\dev\work"));
    assert_eq!(ok("C:/dev/../work/./x"), PathBuf::from(r"C:\work\x"));
    assert_eq!(ok("C:/../.."), PathBuf::from(r"C:\"));
}

#[test]
fn unc_paths_keep_their_prefix() {
    assert_eq!(ok("//server/share"), PathBuf::from(r"\\server\share"));
    assert_eq!(ok("//server/share/dir/sub"), PathBuf::from(r"\\server\share\dir\sub"));
    assert_eq!(ok(r"\\server\share\dir"), PathBuf::from(r"\\server\share\dir"));
    assert!(err("//server").contains("server and a share"));
    assert!(err("//server/share/../..").contains("server and a share"));
}

#[test]
fn spaces_and_quotes_are_allowed() {
    assert_eq!(ok("C:/Program Files/My App"), PathBuf::from(r"C:\Program Files\My App"));
    assert_eq!(ok("  ~/My Documents  "), home().join("My Documents"));
    assert_eq!(ok("\"C:/Program Files\""), PathBuf::from(r"C:\Program Files"));
}

#[test]
fn malformed_paths_are_rejected() {
    assert!(err("C:/bad|name").contains("Invalid character"));
    assert!(err("~/what?").contains("Invalid character"));
    assert!(err("C:/a/b:c").contains("Invalid character"));
    assert!(err("//server/sh*re").contains("Invalid character"));
}

#[test]
fn check_reports_directories_files_and_missing() {
    let dir = std::env::temp_dir().join(format!("quickswitch-test-{}", std::process::id()));
    let sub = dir.join("with space");
    std::fs::create_dir_all(&sub).unwrap();
    let file = dir.join("file.txt");
    std::fs::write(&file, "x").unwrap();

    // Use the temp dir as "home" so ~/ paths land inside it.
    let home = Path::new(&dir);
    assert_eq!(check("~/with space", home), Some(PathCheck::Directory(sub.clone())));
    assert_eq!(check("~/file.txt", home), Some(PathCheck::NotADirectory(file.clone())));
    assert_eq!(check("~/missing", home), Some(PathCheck::NotFound(dir.join("missing"))));
    assert!(
        check("~/missing", home)
            .unwrap()
            .message()
            .unwrap()
            .starts_with("Folder not found")
    );
    assert_eq!(check("plain words", home), None);
    assert!(matches!(check("~/a|b", home), Some(PathCheck::Invalid(_))));

    std::fs::remove_dir_all(&dir).unwrap();
}

fn split(input: &str) -> (PathBuf, String, String) {
    let c = completion(input, &home()).expect("path-like").expect("valid");
    let next = c.complete("Next");
    (c.dir, c.partial, next)
}

#[test]
fn completion_splits_folder_and_partial_name() {
    let (dir, partial, next) = split("~/projec");
    assert_eq!(dir, PathBuf::from(r"C:\Users\alice"));
    assert_eq!(partial, "projec");
    assert_eq!(next, "~/Next/");

    let (dir, partial, next) = split("~/Projects/");
    assert_eq!(dir, PathBuf::from(r"C:\Users\alice\Projects"));
    assert_eq!(partial, "");
    assert_eq!(next, "~/Projects/Next/");

    let (dir, partial, _) = split("C:/dev/wo");
    assert_eq!(dir, PathBuf::from(r"C:\dev"));
    assert_eq!(partial, "wo");
}

#[test]
fn completion_of_a_whole_folder_lists_it() {
    assert_eq!(
        split("~"),
        (PathBuf::from(r"C:\Users\alice"), String::new(), "~/Next/".into())
    );
    assert_eq!(split("D:").0, PathBuf::from(r"D:\"));
    assert_eq!(split("~/a/..").0, PathBuf::from(r"C:\Users\alice"));
}

#[test]
fn completion_keeps_backslash_style() {
    let (_, partial, next) = split(r"C:\dev\wo");
    assert_eq!(partial, "wo");
    assert_eq!(next, r"C:\dev\Next\");
}

#[test]
fn completion_skips_network_paths_and_rejects_bad_input() {
    assert_eq!(
        completion("//server/share/x", &home()),
        Some(Err(UNC_NOT_LISTED.into()))
    );
    assert!(completion("~bob/x", &home()).unwrap().is_err());
    assert!(completion("~/a?b", &home()).unwrap().is_err());
    assert_eq!(completion("projects", &home()), None);
}

#[test]
fn list_dirs_returns_only_folders_sorted() {
    let root = std::env::temp_dir().join(format!("qs-list-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("beta")).unwrap();
    std::fs::create_dir_all(root.join("Alpha")).unwrap();
    std::fs::write(root.join("file.txt"), "x").unwrap();
    let names: Vec<String> = list_dirs(&root).unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, vec!["Alpha", "beta"]);
    assert!(list_dirs(&root.join("missing")).is_err());
    let _ = std::fs::remove_dir_all(&root);
}

/// `%SystemRoot%` as Windows normalizes it, e.g. `C:\WINDOWS`.
fn system_root() -> PathBuf {
    ok(&std::env::var("SystemRoot").expect("SystemRoot is always set"))
}

#[test]
fn leading_environment_variable_is_expanded() {
    for s in ["%SystemRoot%", "%systemroot%", "%SYSTEMROOT%/", r"%SystemRoot%\"] {
        assert!(looks_like_path(s), "{s:?} should be path-like");
        assert_eq!(ok(s), system_root(), "{s:?}");
    }
    assert_eq!(ok("%SystemRoot%/System32/../Fonts"), system_root().join("Fonts"));
}

#[test]
fn environment_variable_errors() {
    assert_eq!(
        err("%QUICKSWITCH_SURELY_UNSET_VAR%/x"),
        "Unknown variable %QUICKSWITCH_SURELY_UNSET_VAR%"
    );
    // USERNAME is set, but is a name, not a folder.
    assert_eq!(err("%USERNAME%"), "%USERNAME% isn't a folder path");
}

#[test]
fn incomplete_or_embedded_percent_is_not_a_variable() {
    for s in ["%appdata", "%", "%%", "%app data%", "%a%b", "50% off"] {
        assert!(!looks_like_path(s), "{s:?} should not be path-like");
    }
    // Mid-path `%` is a literal folder-name character.
    assert_eq!(ok("C:/x/%foo%"), PathBuf::from(r"C:\x\%foo%"));
}

#[test]
fn completion_after_environment_variable_keeps_it_as_typed() {
    let (dir, partial, next) = split("%SystemRoot%/fon");
    assert_eq!(dir, system_root());
    assert_eq!(partial, "fon");
    assert_eq!(next, "%SystemRoot%/Next/");

    let (dir, partial, next) = split("%systemroot%");
    assert_eq!(dir, system_root());
    assert_eq!(partial, "");
    assert_eq!(next, "%systemroot%/Next/");
}

#[test]
fn file_finds_pasted_file_paths() {
    let dir = std::env::temp_dir().join(format!("qs-file-{}", std::process::id()));
    let sub = dir.join("Docs");
    std::fs::create_dir_all(&sub).unwrap();
    let report = sub.join("report v2.pdf");
    std::fs::write(&report, "x").unwrap();
    let home = Path::new(&dir);
    let full = report.to_string_lossy().into_owned();

    // As pasted: plain, with "Copy as path" quotes, with forward slashes.
    assert_eq!(file(&full, home), Some(report.clone()));
    assert_eq!(file(&format!("\"{full}\""), home), Some(report.clone()));
    assert_eq!(file(&full.replace('\\', "/"), home), Some(report.clone()));
    assert_eq!(file("~/Docs/report v2.pdf", home), Some(report.clone()));

    // Folders, missing files, words and network paths are not files.
    assert_eq!(file("~/Docs", home), None);
    assert_eq!(file("~/Docs/missing.pdf", home), None);
    assert_eq!(file("report v2.pdf", home), None);
    assert_eq!(file(r"\\server\share\report.pdf", home), None);

    // The file still splits as folder input, so its folder can be listed.
    let c = completion(&full, home).unwrap().unwrap();
    assert_eq!(c.dir, sub);
    assert_eq!(c.partial, "report v2.pdf");

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn file_item_opens_the_file() {
    let item = file_item(PathBuf::from(r"C:\Users\alice\Docs\report.pdf"), &home());
    assert_eq!(item.kind(), Kind::File);
    assert_eq!(item.title, "report.pdf");
    assert_eq!(item.subtitle, "~/Docs");
    assert_eq!(item.folder(), None);
    // Files are not remembered.
    assert!(item.recent_key.is_empty());
    assert_eq!(
        item.target,
        Target::OpenFile {
            path: PathBuf::from(r"C:\Users\alice\Docs\report.pdf")
        }
    );
}
