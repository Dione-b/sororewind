//! Disk cache for immutable responses from a live RPC.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::OnceCell;

use crate::error::Result;
use crate::transport::{BoxFuture, Transport};

pub(crate) struct Cache<T> {
    inner: T,
    root: PathBuf,
    network: OnceCell<String>,
}

impl<T: Transport> Cache<T> {
    pub(crate) fn new(inner: T, root: PathBuf) -> Self {
        Self {
            inner,
            root,
            network: OnceCell::new(),
        }
    }

    async fn network(&self) -> Result<&str> {
        self.network
            .get_or_try_init(|| async {
                self.inner
                    .request("getNetwork", &serde_json::json!({}))
                    .await
            })
            .await
            .map(String::as_str)
    }
}

impl<T: Transport> Transport for Cache<T> {
    fn request<'a>(&'a self, method: &'a str, params: &'a Value) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            if method == "getNetwork" {
                return self.network().await.map(str::to_owned);
            }
            if !matches!(method, "getTransaction" | "getLedgers") {
                return self.inner.request(method, params).await;
            }
            let Some(passphrase) = self
                .network()
                .await
                .ok()
                .and_then(crate::rpc::cache_passphrase)
            else {
                return self.inner.request(method, params).await;
            };
            let key = serde_json::json!([passphrase, method, params]).to_string();
            let path = self.root.join("v1").join(digest(&key));

            if let Ok(saved) = tokio::fs::read_to_string(&path).await
                && let Some((checksum, body)) = saved.split_once('\n')
                && digest(body) == checksum
                && crate::rpc::immutable_response(method, params, body)
            {
                log::debug!(target: "rpc", "disk cache hit for {method}");
                return Ok(body.to_owned());
            }

            let body = self.inner.request(method, params).await?;
            if crate::rpc::immutable_response(method, params, &body) {
                let saved = format!("{}\n{body}", digest(&body));
                if let Err(error) =
                    tokio::task::spawn_blocking(move || store(&path, saved.as_bytes())).await
                {
                    log::debug!(target: "rpc", "cache write task failed: {error}");
                }
            }
            Ok(body)
        })
    }

    fn origin(&self) -> String {
        self.inner.origin()
    }

    fn cache_dir(&self) -> Option<&Path> {
        Some(&self.root)
    }

    fn fallback_origins(&self) -> Option<String> {
        self.inner.fallback_origins()
    }
}

fn digest(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}

fn store(path: &Path, saved: &[u8]) {
    let write = || {
        let dir = path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir)?;
        let mut file = tempfile::NamedTempFile::new_in(dir)?;
        file.write_all(saved)?;
        file.persist(path).map_err(|error| error.error)?;
        Ok::<_, std::io::Error>(())
    };
    if let Err(error) = write() {
        log::debug!(target: "rpc", "could not write RPC cache: {error}");
    }
}

pub(crate) fn wasm_matches(hash: &[u8; 32], wasm: &[u8]) -> bool {
    let actual: [u8; 32] = Sha256::digest(wasm).into();
    actual == *hash
}

pub(crate) async fn load_wasm(root: &Path, passphrase: &str, hash: &[u8; 32]) -> Option<Vec<u8>> {
    let wasm = tokio::fs::read(wasm_path(root, passphrase, hash))
        .await
        .ok()?;
    wasm_matches(hash, &wasm).then_some(wasm)
}

pub(crate) async fn save_wasm(root: &Path, passphrase: &str, hash: &[u8; 32], wasm: &[u8]) {
    let path = wasm_path(root, passphrase, hash);
    let wasm = wasm.to_vec();
    let _ = tokio::task::spawn_blocking(move || store(&path, &wasm)).await;
}

fn wasm_path(root: &Path, passphrase: &str, hash: &[u8; 32]) -> PathBuf {
    root.join("v1-wasm")
        .join(digest(passphrase))
        .join(hex::encode(hash))
}

pub(crate) fn default_dir() -> Option<PathBuf> {
    fn absolute_var(name: &str) -> Option<PathBuf> {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    }
    absolute_var("XDG_CACHE_HOME")
        .or_else(|| absolute_var("LOCALAPPDATA"))
        .or_else(|| absolute_var("HOME").map(|path| path.join(".cache")))
        .map(|path| path.join("sororewind"))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::transport::FixtureTransport;

    struct Counting {
        fixture: FixtureTransport,
        calls: Arc<Mutex<Vec<String>>>,
        network: &'static str,
    }

    impl Transport for Counting {
        fn request<'a>(
            &'a self,
            method: &'a str,
            params: &'a Value,
        ) -> BoxFuture<'a, Result<String>> {
            self.calls.lock().unwrap().push(method.to_owned());
            Box::pin(async move {
                if method == "getNetwork" && !self.network.is_empty() {
                    let body = self.fixture.request(method, params).await?;
                    let mut response: Value = serde_json::from_str(&body).unwrap();
                    response["result"]["passphrase"] = self.network.into();
                    return Ok(response.to_string());
                }
                self.fixture.request(method, params).await
            })
        }

        fn origin(&self) -> String {
            self.fixture.origin()
        }
    }

    #[tokio::test]
    async fn stores_only_complete_ledger_pages() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct LedgerRpc(Arc<AtomicUsize>);
        impl Transport for LedgerRpc {
            fn request<'a>(
                &'a self,
                method: &'a str,
                _: &'a Value,
            ) -> BoxFuture<'a, Result<String>> {
                Box::pin(async move {
                    if method == "getNetwork" {
                        return Ok(r#"{"result":{"passphrase":"test"}}"#.into());
                    }
                    self.0.fetch_add(1, Ordering::Relaxed);
                    Ok(r#"{"result":{"ledgers":[{"sequence":10,"headerXdr":"header","metadataXdr":"meta"}]}}"#.into())
                })
            }

            fn origin(&self) -> String {
                "https://rpc.example".into()
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        for _ in 0..2 {
            let cached = Cache::new(LedgerRpc(Arc::clone(&count)), dir.path().to_owned());
            let rpc = crate::RpcClient::new(cached);
            assert_eq!(rpc.get_ledgers(10, 1).await.unwrap().len(), 1);
            assert_eq!(rpc.get_ledgers(10, 2).await.unwrap().len(), 1);
        }
        assert_eq!(count.load(Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn wasm_is_keyed_by_hash_and_passphrase_and_verified_when_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let wasm = b"contract wasm";
        let hash = Sha256::digest(wasm).into();
        save_wasm(dir.path(), "test", &hash, wasm).await;
        assert_eq!(
            load_wasm(dir.path(), "test", &hash).await.as_deref(),
            Some(wasm.as_slice())
        );
        assert!(load_wasm(dir.path(), "main", &hash).await.is_none());

        std::fs::write(wasm_path(dir.path(), "test", &hash), "bad wasm").unwrap();
        assert!(load_wasm(dir.path(), "test", &hash).await.is_none());
    }

    #[tokio::test]
    async fn reuses_final_transactions_without_freezing_current_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/contract-error");
        let hash = FixtureTransport::open(&path)
            .await
            .unwrap()
            .hash_for(None)
            .unwrap();
        let request = crate::Request {
            hash,
            network: None,
            wasm_debug: None,
        };
        let calls = Arc::new(Mutex::new(Vec::new()));
        let replay = || async {
            let transport = Counting {
                fixture: FixtureTransport::open(&path).await.unwrap(),
                calls: Arc::clone(&calls),
                network: "",
            };
            let client = crate::rpc::RpcClient::new(Cache::new(transport, dir.path().to_owned()));
            let report = crate::run(&client, &request).await.unwrap();
            let calls = std::mem::take(&mut *calls.lock().unwrap());
            (crate::render::json(&report).unwrap(), calls)
        };

        let (first, first_calls) = replay().await;
        let (second, second_calls) = replay().await;
        assert_eq!(first, second);
        assert!(first_calls.iter().any(|call| call == "getTransaction"));
        assert!(!second_calls.iter().any(|call| call == "getTransaction"));
        assert!(second_calls.iter().any(|call| call == "getLedgerEntries"));

        for file in std::fs::read_dir(dir.path().join("v1")).unwrap() {
            std::fs::write(file.unwrap().path(), "corrupted cache data").unwrap();
        }
        let (refetched, refetched_calls) = replay().await;
        assert_eq!(refetched, first);
        assert!(refetched_calls.iter().any(|call| call == "getTransaction"));

        let fixture = FixtureTransport::open(&path).await.unwrap();
        let client = crate::rpc::RpcClient::new(Cache::new(
            Counting {
                fixture,
                calls: Arc::clone(&calls),
                network: "a different passphrase",
            },
            dir.path().to_owned(),
        ));
        client.get_transaction(&request.hash).await.unwrap();
        assert!(
            calls
                .lock()
                .unwrap()
                .iter()
                .any(|call| call == "getTransaction")
        );
    }
}
