//! Ordered RPC failover for one network.

use serde_json::{Value, json};
use tokio::sync::OnceCell;

use crate::error::{Result, RewindError};
use crate::transport::{BoxFuture, Transport};

struct Endpoint<T> {
    transport: T,
    identity: OnceCell<String>,
}

pub(crate) struct Fallback<T> {
    endpoints: Vec<Endpoint<T>>,
    network: OnceCell<String>,
    expected: Option<String>,
}

impl<T: Transport> Fallback<T> {
    pub(crate) fn new(transports: Vec<T>, expected: Option<String>) -> Self {
        Self {
            endpoints: transports
                .into_iter()
                .map(|transport| Endpoint {
                    transport,
                    identity: OnceCell::new(),
                })
                .collect(),
            network: OnceCell::new(),
            expected,
        }
    }

    async fn request_ordered(&self, method: &str, params: &Value) -> Result<String> {
        let mut tried = Vec::new();
        let mut last_error = None;
        let mut retention_response = None;
        for endpoint in &self.endpoints {
            let origin = endpoint.transport.origin();
            tried.push(origin.clone());
            let result = async {
                let identity = endpoint
                    .identity
                    .get_or_try_init(|| endpoint.transport.request("getNetwork", &NETWORK_PARAMS))
                    .await?;
                let passphrase =
                    crate::rpc::cache_passphrase(identity).ok_or_else(|| RewindError::Rpc {
                        origin: origin.clone(),
                        method: "getNetwork".into(),
                        code: None,
                        detail: "missing network passphrase".into(),
                    })?;
                let selected = match self.expected.as_deref() {
                    Some(expected) => expected,
                    None => {
                        self.network
                            .get_or_init(|| async { passphrase.clone() })
                            .await
                    }
                };
                if passphrase != selected {
                    return Err(RewindError::Rpc {
                        origin: origin.clone(),
                        method: "getNetwork".into(),
                        code: None,
                        detail: "endpoint serves a different network".into(),
                    });
                }
                if method == "getNetwork" {
                    Ok(identity.clone())
                } else {
                    endpoint.transport.request(method, params).await
                }
            }
            .await;
            match result {
                Ok(body) if retention_miss(method, params, &body) => {
                    log::debug!(target: "rpc", "{origin} lacks the requested data for {method}");
                    retention_response.get_or_insert(body);
                }
                Ok(body) => {
                    log::debug!(target: "rpc", "{origin} served {method}");
                    return Ok(body);
                }
                Err(error) if retry_elsewhere(&error) => {
                    log::debug!(target: "rpc", "{origin} could not serve {method}");
                    last_error = Some(error);
                }
                Err(error) => return Err(with_attempts(error, &tried)),
            }
        }
        if let Some(body) = retention_response {
            // The typed client still interprets a retention miss as absence.
            log::debug!(target: "rpc", "tried {} for {method}", tried.join(", "));
            return Ok(body);
        }
        Err(with_attempts(
            last_error.unwrap_or_else(|| RewindError::InvalidRpcUrl {
                reason: "no endpoints configured".into(),
            }),
            &tried,
        ))
    }
}

static NETWORK_PARAMS: std::sync::LazyLock<Value> = std::sync::LazyLock::new(|| json!({}));

impl<T: Transport> Transport for Fallback<T> {
    fn request<'a>(&'a self, method: &'a str, params: &'a Value) -> BoxFuture<'a, Result<String>> {
        Box::pin(self.request_ordered(method, params))
    }

    fn origin(&self) -> String {
        self.endpoints
            .iter()
            .map(|endpoint| endpoint.transport.origin())
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn fallback_origins(&self) -> Option<String> {
        Some(self.origin())
    }
}

fn with_attempts(error: RewindError, tried: &[String]) -> RewindError {
    RewindError::RpcAttempts {
        endpoints: tried.join(", "),
        source: Box::new(error),
    }
}

fn retry_elsewhere(error: &RewindError) -> bool {
    error.is_transient_rpc()
        || error.is_ledger_out_of_range()
        || matches!(error, RewindError::Rpc { method, .. } if method == "getNetwork")
}

fn retention_miss(method: &str, params: &Value, body: &str) -> bool {
    let Ok(response) = serde_json::from_str::<Value>(body) else {
        return false;
    };
    match method {
        "getTransaction" => response["result"]["status"] == "NOT_FOUND",
        "getLedgers" => {
            response["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("must be between the oldest ledger"))
                || response["result"]["ledgers"]
                    .as_array()
                    .is_some_and(|ledgers| {
                        !ledgers
                            .iter()
                            .any(|ledger| ledger["sequence"] == params["startLedger"])
                    })
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy)]
    enum Answer {
        Body(&'static str),
        Unreachable,
        RateLimited,
    }

    struct Fake {
        origin: &'static str,
        network: &'static str,
        answer: Answer,
    }

    impl Transport for Fake {
        fn request<'a>(&'a self, method: &'a str, _: &'a Value) -> BoxFuture<'a, Result<String>> {
            Box::pin(async move {
                if method == "getNetwork" {
                    return Ok(json!({"result":{"passphrase":self.network}}).to_string());
                }
                match self.answer {
                    Answer::Body(body) => Ok(body.into()),
                    Answer::Unreachable => Err(RewindError::RpcUnreachable {
                        url: self.origin(),
                        detail: "connection failed".into(),
                        source: None,
                    }),
                    Answer::RateLimited => Err(RewindError::Rpc {
                        origin: self.origin(),
                        method: method.into(),
                        code: Some(-32005),
                        detail: "rate limited".into(),
                    }),
                }
            })
        }

        fn origin(&self) -> String {
            self.origin.into()
        }
    }

    fn pair(first: Answer, second: Answer, network: &'static str) -> Fallback<Fake> {
        Fallback::new(
            vec![
                Fake {
                    origin: "https://primary.example",
                    network: "test",
                    answer: first,
                },
                Fake {
                    origin: "https://backup.example",
                    network,
                    answer: second,
                },
            ],
            None,
        )
    }

    #[tokio::test]
    async fn tries_backup_after_transport_rate_limit_and_retention_failures() {
        let archived =
            Answer::Body(r#"{"result":{"ledgers":[{"sequence":10}], "status":"FAILED"}}"#);
        for (method, failure) in [
            ("getTransaction", Answer::Unreachable),
            ("getTransaction", Answer::RateLimited),
            (
                "getTransaction",
                Answer::Body(r#"{"result":{"status":"NOT_FOUND"}}"#),
            ),
            ("getLedgers", Answer::Body(r#"{"result":{"ledgers":[]}}"#)),
            (
                "getLedgers",
                Answer::Body(r#"{"error":{"message":"must be between the oldest ledger"}}"#),
            ),
        ] {
            let response = pair(failure, archived, "test")
                .request(method, &json!({"startLedger":10}))
                .await
                .unwrap();
            assert!(response.contains("FAILED"), "{method}");
        }
    }

    #[tokio::test]
    async fn lists_attempted_origins_and_rejects_a_backup_on_another_network() {
        let fallback = pair(
            Answer::RateLimited,
            Answer::Body("unexpected success"),
            "other",
        );
        let error = fallback
            .request("getTransaction", &json!({}))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("https://primary.example, https://backup.example"));
        assert!(error.contains("different network"));
    }
}
