//! Zip builtin: compress / extract on thread pool; callback receives result Map.

#![allow(dead_code)]

use crate::runner::r#async::AsyncBuiltinResult;
use builtin_macro::boyia_class;
use std::fs::{self, File};
use std::io::copy;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use zip::write::{FileOptions, ZipWriter};
use zip::CompressionMethod;
use zip::ZipArchive;

fn file_options<'a>(password: &'a str) -> FileOptions<'a, ()> {
    let base = FileOptions::<()>::default().compression_method(CompressionMethod::Deflated);
    if password.is_empty() {
        base
    } else {
        base.with_aes_encryption(zip::AesMode::Aes256, password)
    }
}

fn run_compress(src: PathBuf, dest_zip: PathBuf, password: String) -> AsyncBuiltinResult {
    let meta = match fs::metadata(&src) {
        Ok(m) => m,
        Err(e) => {
            return AsyncBuiltinResult::Fail {
                message: format!("Zip.compress error: metadata {e}"),
            };
        }
    };

    let dest_file = match File::create(&dest_zip) {
        Ok(f) => f,
        Err(e) => {
            return AsyncBuiltinResult::Fail {
                message: format!("Zip.compress error: create zip {e}"),
            };
        }
    };

    let mut writer = ZipWriter::new(dest_file);
    let opts = file_options(&password);

    let r = if meta.is_file() {
        let name = src
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();
        if let Err(e) = writer.start_file(&name, opts) {
            let _ = writer.finish();
            return AsyncBuiltinResult::Fail {
                message: format!("Zip.compress error: start_file {e}"),
            };
        }
        match File::open(&src) {
            Ok(mut f) => match copy(&mut f, &mut writer) {
                Ok(_) => Ok(()),
                Err(e) => Err(e.to_string()),
            },
            Err(e) => Err(e.to_string()),
        }
    } else if meta.is_dir() {
        compress_dir(&mut writer, &src, opts)
    } else {
        Err("Zip.compress error: unsupported path type".into())
    };

    if let Err(msg) = r {
        let _ = writer.finish();
        return AsyncBuiltinResult::Fail { message: msg };
    }

    match writer.finish() {
        Ok(_) => AsyncBuiltinResult::Ok { data: None },
        Err(e) => AsyncBuiltinResult::Fail {
            message: format!("Zip.compress error: finish {e}"),
        },
    }
}

fn compress_dir<'a>(
    writer: &mut ZipWriter<File>,
    root: &Path,
    opts: FileOptions<'a, ()>,
) -> Result<(), String> {
    for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file() {
            let rel = path.strip_prefix(root).map_err(|e| e.to_string())?;
            let name = rel
                .to_str()
                .ok_or_else(|| "Zip.compress error: non-utf8 path".to_string())?
                .replace('\\', "/");
            writer
                .start_file(&name, opts.clone())
                .map_err(|e| format!("Zip.compress error: start_file {e}"))?;
            let mut f = File::open(path).map_err(|e| e.to_string())?;
            copy(&mut f, writer).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn open_entry<'a>(
    archive: &'a mut ZipArchive<File>,
    i: usize,
    password: &str,
) -> Result<zip::read::ZipFile<'a>, String> {
    if password.is_empty() {
        archive.by_index(i).map_err(|e| e.to_string())
    } else {
        archive
            .by_index_decrypt(i, password.as_bytes())
            .map_err(|e| e.to_string())
    }
}

fn run_extract(src_zip: PathBuf, dest_dir: PathBuf, password: String) -> AsyncBuiltinResult {
    let file = match File::open(&src_zip) {
        Ok(f) => f,
        Err(e) => {
            return AsyncBuiltinResult::Fail {
                message: format!("Zip.extract error: open {e}"),
            };
        }
    };

    let mut archive = match ZipArchive::new(file) {
        Ok(a) => a,
        Err(e) => {
            return AsyncBuiltinResult::Fail {
                message: format!("Zip.extract error: {e}"),
            };
        }
    };

    if let Err(e) = fs::create_dir_all(&dest_dir) {
        return AsyncBuiltinResult::Fail {
            message: format!("Zip.extract error: mkdir {e}"),
        };
    }

    for i in 0..archive.len() {
        let mut entry = match open_entry(&mut archive, i, &password) {
            Ok(e) => e,
            Err(e) => {
                return AsyncBuiltinResult::Fail {
                    message: format!("Zip.extract error: entry {i} {e}"),
                };
            }
        };

        let enclosed = match entry.enclosed_name() {
            Some(p) => p.to_owned(),
            None => continue,
        };

        let out_path = dest_dir.join(&enclosed);
        if entry.is_dir() {
            if let Err(e) = fs::create_dir_all(&out_path) {
                return AsyncBuiltinResult::Fail {
                    message: format!("Zip.extract error: mkdir {e}"),
                };
            }
            continue;
        }

        if let Some(parent) = out_path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                return AsyncBuiltinResult::Fail {
                    message: format!("Zip.extract error: mkdir {e}"),
                };
            }
        }

        let mut out_file = match File::create(&out_path) {
            Ok(f) => f,
            Err(e) => {
                return AsyncBuiltinResult::Fail {
                    message: format!("Zip.extract error: create file {e}"),
                };
            }
        };

        if let Err(e) = copy(&mut entry, &mut out_file) {
            return AsyncBuiltinResult::Fail {
                message: format!("Zip.extract error: write {e}"),
            };
        }
    }

    AsyncBuiltinResult::Ok { data: None }
}

struct ZipBuiltins;

#[boyia_class(name = "Zip", registrar = builtin_zip_class)]
impl ZipBuiltins {
    #[boyia_async_builtin(native = zip_compress_native, method = "compress")]
    fn zip_compress(
        src: String,
        dest: String,
        #[optional(default = "")]
        password: String,
    ) -> AsyncBuiltinResult {
        run_compress(PathBuf::from(src), PathBuf::from(dest), password)
    }

    #[boyia_async_builtin(native = zip_extract_native, method = "extract")]
    fn zip_extract(
        src: String,
        dest: String,
        #[optional(default = "")]
        password: String,
    ) -> AsyncBuiltinResult {
        run_extract(PathBuf::from(src), PathBuf::from(dest), password)
    }
}
