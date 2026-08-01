#![cfg(windows)]

use std::ffi::{OsStr, OsString};
use std::io;
use std::mem::{offset_of, size_of};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::AsRawHandle;
use std::path::PathBuf;

use cap_std::fs::{Dir, File, OpenOptions, OpenOptionsExt};
use windows_sys::Win32::Foundation::GENERIC_WRITE;
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_RENAME_INFO, FILE_RENAME_INFO_0, FileRenameInfo, GetFinalPathNameByHandleW,
    SetFileInformationByHandle, VOLUME_NAME_DOS,
};

/// Grants a newly-created staging file the access required for handle-relative
/// publication. The caller must still request write and create-new behavior.
pub fn prepare_rename_source(options: &mut OpenOptions) {
    options.access_mode(GENERIC_WRITE | DELETE);
}

/// Atomically replaces one entry in `parent` by renaming an already-open
/// staging file to a path resolved from the retained parent directory handle.
///
/// # Errors
///
/// Returns the Windows error reported by `SetFileInformationByHandle`.
pub fn atomic_replace(source: &File, parent: &Dir, destination: &OsStr) -> io::Result<()> {
    let mut destination_path = final_path(parent)?;
    destination_path.push(destination);
    let name: Vec<u16> = destination_path.as_os_str().encode_wide().collect();
    let name_bytes = name
        .len()
        .checked_mul(size_of::<u16>())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file name is too long"))?;
    let buffer_bytes = offset_of!(FILE_RENAME_INFO, FileName)
        .checked_add(name_bytes)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file name is too long"))?;
    let words = buffer_bytes.div_ceil(size_of::<usize>());
    let mut storage = vec![0usize; words];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();

    // SAFETY: `storage` is pointer-aligned and sized for the fixed header plus
    // `name_bytes`. Both handles are borrowed and valid for the duration of the
    // call. `FileNameLength` excludes a terminator, as required by Win32.
    unsafe {
        info.write(FILE_RENAME_INFO {
            Anonymous: FILE_RENAME_INFO_0 {
                ReplaceIfExists: true,
            },
            RootDirectory: std::ptr::null_mut(),
            FileNameLength: u32::try_from(name_bytes).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "file name is too long")
            })?,
            FileName: [0],
        });
        std::ptr::copy_nonoverlapping(name.as_ptr(), (*info).FileName.as_mut_ptr(), name.len());
        let result = SetFileInformationByHandle(
            source.as_raw_handle(),
            FileRenameInfo,
            info.cast(),
            u32::try_from(buffer_bytes).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "file name is too long")
            })?,
        );
        if result == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

fn final_path(directory: &Dir) -> io::Result<PathBuf> {
    let mut capacity = 512_usize;
    loop {
        let mut buffer = vec![0_u16; capacity];
        // SAFETY: `buffer` is writable for `capacity` UTF-16 code units and the
        // directory handle remains valid for the duration of the call.
        let length = unsafe {
            GetFinalPathNameByHandleW(
                directory.as_raw_handle(),
                buffer.as_mut_ptr(),
                u32::try_from(capacity).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "directory path is too long")
                })?,
                VOLUME_NAME_DOS,
            )
        };
        if length == 0 {
            return Err(io::Error::last_os_error());
        }
        let length = usize::try_from(length).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "directory path length is invalid",
            )
        })?;
        if length < capacity {
            buffer.truncate(length);
            return Ok(PathBuf::from(OsString::from_wide(&buffer)));
        }
        capacity = length.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write as _;

    #[test]
    fn atomic_replace_publishes_over_an_existing_file() {
        let temporary = tempfile::tempdir().unwrap();
        fs::write(temporary.path().join("target.txt"), "old").unwrap();
        let parent = Dir::open_ambient_dir(temporary.path(), cap_std::ambient_authority()).unwrap();
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        prepare_rename_source(&mut options);
        let mut source = parent.open_with("staged.txt", &options).unwrap();
        source.write_all(b"new").unwrap();
        source.sync_all().unwrap();

        atomic_replace(&source, &parent, OsStr::new("target.txt")).unwrap();

        assert_eq!(
            fs::read_to_string(temporary.path().join("target.txt")).unwrap(),
            "new"
        );
        assert!(!temporary.path().join("staged.txt").exists());
    }
}
