use std::fs::{create_dir, File};
use std::path::{Path, PathBuf};

use log::trace;

use serial_test::serial;
use trash::{delete, delete_all};

mod util {
    use std::sync::atomic::{AtomicI32, Ordering};

    use once_cell::sync::Lazy;

    // WARNING Expecting that `cargo test` won't be invoked on the same computer more than once within
    // a single millisecond
    static INSTANCE_ID: Lazy<i64> = Lazy::new(|| chrono::Local::now().timestamp_millis());
    static ID_OFFSET: AtomicI32 = AtomicI32::new(0);
    pub fn get_unique_name() -> String {
        let id = ID_OFFSET.fetch_add(1, Ordering::SeqCst);
        format!("trash-test-{}-{}", *INSTANCE_ID, id)
    }

    pub fn init_logging() {
        let _ = env_logger::builder().is_test(true).try_init();
    }
}
pub use util::{get_unique_name, init_logging};

#[test]
#[serial]
fn test_delete_file() {
    init_logging();
    trace!("Started test_delete_file");

    let path = get_unique_name();
    File::create_new(&path).unwrap();

    delete(&path).unwrap();
    assert!(File::open(&path).is_err());
    trace!("Finished test_delete_file");
}

#[test]
#[serial]
fn test_delete_folder() {
    init_logging();
    trace!("Started test_delete_folder");

    let path = PathBuf::from(get_unique_name());
    create_dir(&path).unwrap();
    File::create_new(path.join("file_in_folder")).unwrap();

    assert!(path.exists());
    delete(&path).unwrap();
    assert!(!path.exists());

    trace!("Finished test_delete_folder");
}

#[test]
fn test_delete_all() {
    init_logging();
    trace!("Started test_delete_all");
    let count: usize = 3;

    let paths: Vec<_> = (0..count).map(|i| format!("test_file_to_delete_{i}")).collect();
    for path in paths.iter() {
        File::create_new(path).unwrap();
    }

    delete_all(&paths).unwrap();
    for path in paths.iter() {
        assert!(File::open(path).is_err());
    }
    trace!("Finished test_delete_all");
}

#[cfg(unix)]
mod unix {
    use log::trace;
    use std::{
        fs::{create_dir, remove_dir_all, remove_file, File},
        os::unix::fs::symlink,
        path::Path,
    };

    use super::{get_unique_name, init_logging};
    use crate::delete;
    // use crate::init_logging;

    #[test]
    #[ignore = "permission denied in more recent macOS versions"]
    fn test_delete_symlink() {
        init_logging();
        trace!("Started test_delete_symlink");
        let target_path = get_unique_name();
        File::create_new(&target_path).unwrap();

        let link_path = "test_link_to_delete";
        symlink(&target_path, link_path).unwrap();

        delete(link_path).unwrap();
        assert!(File::open(link_path).is_err());
        assert!(File::open(&target_path).is_ok());
        // Cleanup
        remove_file(&target_path).unwrap();
        trace!("Finished test_delete_symlink");
    }

    #[test]
    #[ignore = "permission denied in more recent macOS versions"]
    fn test_delete_symlink_in_folder() {
        init_logging();
        trace!("Started test_delete_symlink_in_folder");
        let target_path = "test_link_target_for_delete_from_folder";
        File::create_new(target_path).unwrap();

        let folder = Path::new("test_parent_folder_for_link_to_delete");
        create_dir(folder).unwrap();
        let link_path = folder.join("test_link_to_delete_from_folder");
        symlink(target_path, &link_path).unwrap();

        delete(&link_path).unwrap();
        assert!(File::open(link_path).is_err());
        assert!(File::open(target_path).is_ok());
        // Cleanup
        remove_file(target_path).unwrap();
        remove_dir_all(folder).unwrap();
        trace!("Finished test_delete_symlink_in_folder");
    }
}

#[test]
#[serial]
fn create_remove_single_file() {
    // Let's create and remove a single file
    let name = get_unique_name();
    File::create_new(&name).unwrap();
    trash::delete(&name).unwrap();
    assert!(File::open(&name).is_err());
}

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "ios"), not(target_os = "android")))]
#[test]
#[serial]
fn create_remove_single_file_invalid_utf8() {
    use std::ffi::OsStr;
    let name = unsafe { OsStr::from_encoded_bytes_unchecked(&[168]) };
    File::create_new(name).unwrap();
    trash::delete(name).unwrap();
}

#[test]
fn recursive_file_deletion() {
    let parent_dir = Path::new("remove-me");
    let dir1 = parent_dir.join("dir1");
    let dir2 = parent_dir.join("dir2");
    std::fs::create_dir_all(&dir1).unwrap();
    std::fs::create_dir_all(&dir2).unwrap();
    File::create_new(dir1.join("same-name")).unwrap();
    File::create_new(dir2.join("same-name")).unwrap();

    trash::delete(parent_dir).unwrap();
    assert!(!parent_dir.exists());
}

#[test]
fn recursive_file_with_content_deletion() {
    let parent_dir = Path::new("remove-me-content");
    let dir1 = parent_dir.join("dir1");
    let dir2 = parent_dir.join("dir2");
    std::fs::create_dir_all(&dir1).unwrap();
    std::fs::create_dir_all(&dir2).unwrap();
    File::create_new(dir1.join("same-name")).unwrap();
    std::fs::write(dir2.join("same-name"), b"some content").unwrap();

    trash::delete(parent_dir).unwrap();
    assert!(!parent_dir.exists());
}

#[test]
#[serial]
#[cfg(target_os = "linux")]
fn test_delete_with_info() {
    // Create the test file to be deleted, ensuring that we include the current
    // directory so we can later assert that the `original_parent` is preserved.
    let path = std::env::current_dir().expect("Should be able to get current directory").join(get_unique_name());
    File::create_new(&path).unwrap();

    // Create a new trash context for deleting the file with info.
    let trash = trash::TrashContext::new();

    match trash.delete_with_info(&path) {
        Ok(trash_item) => {
            // Before asserting any of the fields, we'll go ahead and remove the
            // trashed file from the Freedesktop trash, otherwise it'll be kept
            // around after the test is finished.
            // On Freedesktop, `id` points to the `.trashinfo` file. The actual
            // trashed file lives in the sibling `files/` directory, so we need
            // to clean up both.
            let info_path = PathBuf::from(&trash_item.id);
            let _ = std::fs::remove_file(&info_path);
            if let Some(info_dir) = info_path.parent().and_then(|p| p.parent()) {
                let file_in_trash = info_dir.join("files").join(info_path.file_stem().unwrap_or_default());
                let _ = std::fs::remove_file(&file_in_trash).or_else(|_| std::fs::remove_dir_all(&file_in_trash));
            }

            assert_eq!(trash_item.name, path.components().last().expect("Should have last component").as_os_str());
            assert_eq!(trash_item.original_parent, path.parent().expect("Should have parent").as_os_str());
        }
        _ => panic!("Calling delete_with_info failed to return TrashItem."),
    }
}

#[test]
#[serial]
#[cfg(target_os = "windows")]
fn test_delete_with_info() {
    use trash::os_limited::purge_all;

    // Create the test file to be deleted, ensuring that we include the current
    // directory so we can later assert that the `original_parent` is preserved.
    let path = std::env::current_dir().expect("Should be able to get current directory").join(get_unique_name());
    File::create_new(&path).unwrap();

    // Create a new trash context for deleting the file with info.
    let trash = trash::TrashContext::new();

    match trash.delete_with_info(&path) {
        Ok(trash_item) => {
            // Before asserting any of the fields, we'll go ahead and remove the
            // trashed file from the Recycle Bin, otherwise it'll be kept around
            // after the test is finished.
            // The returned `Result` is ignored, as we don't want the test to
            // fail in case we're not able to remove the file from trash.
            let _ = purge_all([&trash_item]);

            assert_eq!(trash_item.name, path.components().last().expect("Should have last component").as_os_str());
            assert_eq!(trash_item.original_parent, path.parent().expect("Should have parent"));
        }
        _ => panic!("Calling delete_with_info failed to return TrashItem."),
    }
}
