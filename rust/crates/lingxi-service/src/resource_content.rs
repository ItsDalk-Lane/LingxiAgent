//! 下载只从已打开的同一文件分块读取，背压和断连都不会触发全量预读。
use super::{ResolvedContent, ResourceError};
use crate::filetools::fsio::{DirFd, FileIdentity};
use std::{
    future::Future,
    io::{self, Read, Seek, SeekFrom},
    path::PathBuf,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

const CHUNK_BYTES: u64 = 64 * 1024;
struct Reader {
    file: std::fs::File,
    identity: FileIdentity,
    dir: DirFd,
    name: std::ffi::OsString,
    path: PathBuf,
    remaining: u64,
    // 清理租约覆盖真实工作线程与整个正文，不在返回HTTP头时提前释放。
    _lease: tokio::sync::OwnedRwLockReadGuard<()>,
}
impl Reader {
    fn validate(&self) -> io::Result<()> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| io::Error::other("resource_content_changed"))?;
        if !self.dir.same_directory(parent)? || FileIdentity::of(&self.file)? != self.identity {
            return Err(io::Error::other("resource_content_changed"));
        }
        let current = self
            .dir
            .open_attachment_nofollow(&self.name)
            .ok_or_else(|| io::Error::other("resource_content_changed"))??;
        if FileIdentity::of(&current)? != self.identity {
            return Err(io::Error::other("resource_content_changed"));
        }
        Ok(())
    }
    fn chunk(&mut self) -> io::Result<Vec<u8>> {
        self.validate()?;
        let mut bytes = vec![0; self.remaining.min(CHUNK_BYTES) as usize];
        self.file.read_exact(&mut bytes)?;
        self.validate()?;
        self.remaining -= bytes.len() as u64;
        Ok(bytes)
    }
}
pub(super) struct ContentBody {
    reader: Arc<Mutex<Reader>>,
    task: Option<tokio::task::JoinHandle<io::Result<Vec<u8>>>>,
    remaining: u64,
    failed: bool,
}
impl axum::body::HttpBody for ContentBody {
    type Data = axum::body::Bytes;
    type Error = io::Error;
    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<hyper::body::Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        if this.failed || this.remaining == 0 {
            return Poll::Ready(None);
        }
        if this.task.is_none() {
            let reader = Arc::clone(&this.reader);
            this.task = Some(tokio::task::spawn_blocking(move || {
                reader.lock().unwrap_or_else(|p| p.into_inner()).chunk()
            }));
        }
        let result = match Pin::new(this.task.as_mut().unwrap()).poll(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(result) => result,
        };
        this.task = None;
        match result {
            Ok(Ok(bytes)) => {
                this.remaining -= bytes.len() as u64;
                let frame = hyper::body::Frame::data(axum::body::Bytes::from(bytes));
                Poll::Ready(Some(Ok(frame)))
            }
            Ok(Err(error)) => {
                this.failed = true;
                Poll::Ready(Some(Err(error)))
            }
            Err(error) => {
                this.failed = true;
                Poll::Ready(Some(Err(io::Error::other(error.to_string()))))
            }
        }
    }
    fn is_end_stream(&self) -> bool {
        self.failed || self.remaining == 0
    }
    fn size_hint(&self) -> hyper::body::SizeHint {
        hyper::body::SizeHint::with_exact(self.remaining)
    }
}

pub(super) fn prepare(
    content: &ResolvedContent,
    start: u64,
    length: u64,
    lease: tokio::sync::OwnedRwLockReadGuard<()>,
    observer: Option<Arc<dyn Fn() + Send + Sync>>,
) -> Result<ContentBody, ResourceError> {
    let result = (|| -> io::Result<Reader> {
        if let Some(observer) = observer {
            observer();
        }
        let path = &content.file_path;
        if path.canonicalize()?.as_path() != path {
            return Err(io::Error::other("resource_content_changed"));
        }
        let dir = DirFd::open(
            path.parent()
                .ok_or_else(|| io::Error::other("invalid resource parent"))?,
        )?;
        let name = path
            .file_name()
            .ok_or_else(|| io::Error::other("invalid resource name"))?
            .to_os_string();
        let mut file = dir
            .open_attachment_nofollow(&name)
            .ok_or_else(|| io::Error::other("resource_content_changed"))??;
        if !file.metadata()?.is_file() {
            return Err(io::Error::other("resource_not_file"));
        }
        let identity = FileIdentity::of(&file)?;
        let modified = identity
            .modified
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        if identity.size != content.size || modified != content.mtime_ms {
            return Err(io::Error::other("resource_content_changed"));
        }
        file.seek(SeekFrom::Start(start))?;
        let reader = Reader {
            file,
            identity,
            dir,
            name,
            path: path.clone(),
            remaining: length,
            _lease: lease,
        };
        reader.validate()?;
        Ok(reader)
    })();
    let reader = result.map_err(|e| {
        ResourceError::new(
            409,
            "resource_content_changed",
            format!("resource content changed before reading: {e}"),
        )
    })?;
    Ok(ContentBody {
        reader: Arc::new(Mutex::new(reader)),
        task: None,
        remaining: length,
        failed: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::HttpBody;
    #[tokio::test]
    async fn actual_sparse_file_first_frame_is_bounded_and_drop_releases_payload_lease() {
        let root = std::env::temp_dir().join(format!(
            "r06-content-frames-{}",
            crate::auth::hex_random_public(8).unwrap()
        ));
        std::fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let path = root.join("large.bin");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(64 * 1024 * 1024).unwrap();
        drop(file);
        let meta = std::fs::metadata(&path).unwrap();
        let content = ResolvedContent {
            resource_id: "res_sf_frame".into(),
            file_path: path,
            mime: "application/octet-stream".into(),
            size: meta.len(),
            filename: "large.bin".into(),
            mtime_ms: meta
                .modified()
                .unwrap()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            etag: "test".into(),
        };
        let lock = Arc::new(tokio::sync::RwLock::new(()));
        let lease = lock.clone().read_owned().await;
        let mut body = prepare(&content, 0, content.size, lease, None).unwrap();
        assert!(lock.try_write().is_err());
        let frame = std::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
            .await
            .unwrap()
            .unwrap()
            .into_data()
            .unwrap();
        assert_eq!(frame.len(), 64 * 1024);
        assert_eq!(body.remaining, content.size - 64 * 1024);
        assert_eq!(
            body.reader.lock().unwrap().file.stream_position().unwrap(),
            64 * 1024
        );
        #[cfg(unix)]
        {
            let outside = root.join("outside.bin");
            std::fs::write(&outside, b"PRIVATE-SENTINEL").unwrap();
            std::fs::rename(&content.file_path, root.join("retained-original.bin")).unwrap();
            std::os::unix::fs::symlink(&outside, &content.file_path).unwrap();
            let next = std::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
                .await
                .unwrap();
            assert!(next.is_err());
            assert_eq!(std::fs::read(&outside).unwrap(), b"PRIVATE-SENTINEL");
        }
        drop(body);
        assert!(lock.try_write().is_ok());
        assert_eq!(
            std::fs::metadata(root.join("retained-original.bin"))
                .or_else(|_| std::fs::metadata(&content.file_path))
                .unwrap()
                .len(),
            64 * 1024 * 1024
        );
    }
}
