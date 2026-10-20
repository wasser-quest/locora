//! The HTTP exchanges of the cloud client, as data. A flow asks for a `Call`, and it is answered
//! with the `Reply`. A flow never does I/O itself: the desktop and the CLI run a flow with `run`
//! (ureq), and the browser runs the same flow with `fetch`, one call at a time, so nothing blocks.

use anyhow::{Context, Result, bail};
use serde::de::DeserializeOwned;

/// Largest response body accepted from the server (one media file or one page of operations).
pub const MAX_BODY: usize = 512 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Head,
    Post,
    Put,
    Delete,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Head => "HEAD",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    /// Sent as `application/json`.
    Json(Vec<u8>),
    /// Sent as `application/octet-stream`.
    Bytes(Vec<u8>),
}

/// One request: a full URL (the server's address plus the path), the bearer token, and a body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub method: Method,
    pub url: String,
    pub token: Option<String>,
    pub body: Option<Body>,
    /// Seconds before the call gives up (media transfers take longer than a sign-in).
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// What a flow does next: send a call and wait for its reply, or finish with its output.
pub enum Step<T> {
    Call(Call),
    Done(T),
}

/// A cloud operation as steps. The first `step` gets `None`; every later one gets the reply to the
/// call the previous step asked for.
pub trait Flow {
    type Output;

    fn step(&mut self, reply: Option<Reply>) -> Result<Step<Self::Output>>;
}

/// Runs a flow with the desktop HTTP client until it is done.
#[cfg(feature = "cloud")]
pub fn run<F: Flow>(mut flow: F) -> Result<F::Output> {
    let mut reply = None;
    loop {
        match flow.step(reply.take())? {
            Step::Done(output) => return Ok(output),
            Step::Call(call) => reply = Some(native::perform(&call)?),
        }
    }
}

/// The body of a successful reply as JSON (an empty body reads as `null`), or the server's message.
pub fn json<T: DeserializeOwned>(reply: Reply) -> Result<T> {
    if !reply.is_success() {
        return Err(failure(&reply));
    }
    if reply.body.is_empty() {
        return Ok(serde_json::from_str("null")?);
    }
    serde_json::from_slice(&reply.body).context("Antwort von Locora Cloud ist ungültig")
}

/// The server's error message for a failed reply.
pub fn failure(reply: &Reply) -> anyhow::Error {
    #[derive(serde::Deserialize)]
    struct ErrorBody {
        error: String,
    }
    // Status 0 is a reply that never came: the page could not reach the server.
    if reply.status == 0 {
        return anyhow::anyhow!("Locora Cloud nicht erreichbar");
    }
    match serde_json::from_slice::<ErrorBody>(&reply.body) {
        Ok(body) => anyhow::anyhow!("{}", body.error),
        Err(_) => anyhow::anyhow!("Locora Cloud antwortet mit {}", reply.status),
    }
}

/// A single request whose reply is read by `read`.
pub struct Exchange<T> {
    call: Option<Call>,
    read: fn(Reply) -> Result<T>,
}

impl<T> Exchange<T> {
    pub fn new(call: Call, read: fn(Reply) -> Result<T>) -> Self {
        Self { call: Some(call), read }
    }
}

impl<T> Flow for Exchange<T> {
    type Output = T;

    fn step(&mut self, reply: Option<Reply>) -> Result<Step<T>> {
        match reply {
            None => match self.call.take() {
                Some(call) => Ok(Step::Call(call)),
                None => bail!("the request was already sent"),
            },
            Some(reply) => Ok(Step::Done((self.read)(reply)?)),
        }
    }
}

/// Checks that a reply is a success, and returns nothing else.
pub fn expect_success(reply: Reply) -> Result<()> {
    if reply.is_success() { Ok(()) } else { Err(failure(&reply)) }
}

#[cfg(feature = "cloud")]
mod native {
    use super::*;
    use std::time::Duration;

    /// Sends one call with ureq. A non-2xx status is a reply like any other.
    pub fn perform(call: &Call) -> Result<Reply> {
        let mut request = ureq::http::Request::builder()
            .method(call.method.as_str())
            .uri(&call.url);
        if let Some(token) = &call.token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let request = match &call.body {
            Some(Body::Json(bytes)) => request
                .header("content-type", "application/json")
                .body(bytes.clone())?,
            Some(Body::Bytes(bytes)) => request
                .header("content-type", "application/octet-stream")
                .body(bytes.clone())?,
            None => request.body(Vec::new())?,
        };
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(call.timeout_secs)))
            .http_status_as_error(false)
            .max_redirects(0)
            .build()
            .into();
        let mut response = agent
            .run(request)
            .context("Locora Cloud nicht erreichbar")?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(MAX_BODY as u64)
            .read_to_vec()?;
        Ok(Reply { status, body })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_reads_a_success_and_the_server_message_of_a_failure() {
        let ok: serde_json::Value = json(Reply { status: 200, body: br#"{"a":1}"#.to_vec() }).unwrap();
        assert_eq!(ok["a"], 1);
        let empty: Option<u8> = json(Reply { status: 204, body: Vec::new() }).unwrap();
        assert_eq!(empty, None);
        let failed = json::<serde_json::Value>(Reply { status: 403, body: br#"{"error":"no access"}"#.to_vec() });
        assert_eq!(failed.unwrap_err().to_string(), "no access");
        let plain = json::<serde_json::Value>(Reply { status: 500, body: b"oops".to_vec() });
        assert_eq!(plain.unwrap_err().to_string(), "Locora Cloud antwortet mit 500");
    }

    struct Echo;

    impl Flow for Echo {
        type Output = u16;

        fn step(&mut self, reply: Option<Reply>) -> Result<Step<u16>> {
            match reply {
                None => Ok(Step::Call(Call {
                    method: Method::Get,
                    url: "https://example.invalid/v1/x".into(),
                    token: None,
                    body: None,
                    timeout_secs: 20,
                })),
                Some(reply) => Ok(Step::Done(reply.status)),
            }
        }
    }

    #[test]
    fn a_flow_asks_for_its_call_then_finishes_with_the_reply() {
        let mut flow = Echo;
        let call = match flow.step(None).unwrap() {
            Step::Call(call) => call,
            Step::Done(_) => panic!("the flow must ask first"),
        };
        assert_eq!(call.method.as_str(), "GET");
        assert!(matches!(flow.step(Some(Reply { status: 418, body: Vec::new() })), Ok(Step::Done(418))));
    }
}
