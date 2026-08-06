//! N4: OPFS persistence — redb's `StorageBackend` over a
//! `FileSystemSyncAccessHandle`. The six sync trait methods map 1:1 onto the
//! handle (`getSize`/`read`/`truncate`/`flush`/`write`/`close`), which is the
//! same shape SQLite-WASM's OPFS VFS uses.
//!
//! Constraints this module encodes rather than documents away:
//! - Sync access handles exist ONLY in dedicated workers — never the main
//!   thread, never a Service Worker. `acquire` fails with a clear error
//!   elsewhere.
//! - A sync access handle is EXCLUSIVE per file (N4b): the second opener gets
//!   a clear "another tab/worker owns this store" error — exactly the native
//!   file mode's lock refusal (K10), surfaced instead of corrupting.
//! - redb demands `Send + Sync` of the backend; JS handles are neither.
//!   `send_wrapper` provides both soundly for this single-threaded target
//!   (runtime-checked — a cross-thread touch panics instead of racing).

use send_wrapper::SendWrapper;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

pub struct OpfsBackend {
    handle: SendWrapper<web_sys::FileSystemSyncAccessHandle>,
}

impl std::fmt::Debug for OpfsBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OpfsBackend")
    }
}

fn io_err(op: &str, e: JsValue) -> std::io::Error {
    std::io::Error::other(format!("opfs {op}: {e:?}"))
}

impl OpfsBackend {
    /// Open (or create) `name` in the origin-private file system and take its
    /// exclusive sync access handle. Dedicated-worker-only by platform rule.
    pub async fn acquire(name: &str) -> Result<Self, String> {
        let global = js_sys::global();
        let scope: web_sys::WorkerGlobalScope = global
            .dyn_into()
            .map_err(|_| {
                "OPFS persistence needs a dedicated worker: sync access handles do not exist on \
                 the main thread or in a Service Worker"
                    .to_owned()
            })?;
        let storage = scope.navigator().storage();
        let dir: web_sys::FileSystemDirectoryHandle =
            JsFuture::from(storage.get_directory())
                .await
                .map_err(|e| format!("origin-private file system unavailable: {e:?}"))?
                .into();
        let opts = web_sys::FileSystemGetFileOptions::new();
        opts.set_create(true);
        let file: web_sys::FileSystemFileHandle =
            JsFuture::from(dir.get_file_handle_with_options(name, &opts))
                .await
                .map_err(|e| format!("opening {name}: {e:?}"))?
                .into();
        let handle: web_sys::FileSystemSyncAccessHandle =
            JsFuture::from(file.create_sync_access_handle())
                .await
                .map_err(|_| {
                    format!(
                        "another tab or worker already owns the store file {name:?} — the sync \
                         access handle is exclusive (N4b), exactly like the native file lock \
                         (K10). Close the other tab, or run this one without persistence."
                    )
                })?
                .into();
        Ok(Self {
            handle: SendWrapper::new(handle),
        })
    }
}

impl redb::StorageBackend for OpfsBackend {
    fn len(&self) -> Result<u64, std::io::Error> {
        self.handle
            .get_size()
            .map(|s| s as u64)
            .map_err(|e| io_err("getSize", e))
    }

    fn read(&self, offset: u64, out: &mut [u8]) -> Result<(), std::io::Error> {
        let mut done = 0usize;
        while done < out.len() {
            let opts = web_sys::FileSystemReadWriteOptions::new();
            opts.set_at((offset + done as u64) as f64);
            let n = self
                .handle
                .read_with_u8_array_and_options(&mut out[done..], &opts)
                .map_err(|e| io_err("read", e))? as usize;
            if n == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "opfs read past end of file",
                ));
            }
            done += n;
        }
        Ok(())
    }

    fn set_len(&self, len: u64) -> Result<(), std::io::Error> {
        // OPFS truncate both shrinks and zero-extends — matching the trait's
        // "new positions must be zero" contract.
        self.handle
            .truncate_with_f64(len as f64)
            .map_err(|e| io_err("truncate", e))
    }

    fn sync_data(&self) -> Result<(), std::io::Error> {
        self.handle.flush().map_err(|e| io_err("flush", e))
    }

    fn write(&self, offset: u64, data: &[u8]) -> Result<(), std::io::Error> {
        let mut done = 0usize;
        while done < data.len() {
            let opts = web_sys::FileSystemReadWriteOptions::new();
            opts.set_at((offset + done as u64) as f64);
            let n = self
                .handle
                .write_with_u8_array_and_options(&data[done..], &opts)
                .map_err(|e| io_err("write", e))? as usize;
            if n == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "opfs write made no progress",
                ));
            }
            done += n;
        }
        Ok(())
    }

    fn close(&self) -> Result<(), std::io::Error> {
        self.handle.close();
        Ok(())
    }
}
