//! The HTTP exchanges of the cloud client, as data. A flow asks for a `Call`, and it is answered
//! with the `Reply`. A flow never does I/O itself: the desktop and the CLI run a flow with `run`
//! (ureq), and the browser runs the same flow with `fetch`, one call at a time, so nothing blocks.

use anyhow::{Context, Result, bail};
use locora_cloud_api::api::{ErrorBody, REASON_CONFIRM_EMAIL};
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

/// Calls a flow may ask for at once (media transfers); the runners send them side by side.
pub const PARALLEL: usize = 4;

/// What a flow does next: send a call and wait for its reply, send several calls at once and wait
/// for all their replies (`step_many`, in the same order), or finish with its output.
pub enum Step<T> {
    Call(Call),
    Calls(Vec<Call>),
    Done(T),
}

/// A cloud operation as steps. The first `step` gets `None`; every later one gets the reply to the
/// call the previous step asked for, or `step_many` the replies to its calls.
pub trait Flow {
    type Output;

    fn step(&mut self, reply: Option<Reply>) -> Result<Step<Self::Output>>;

    /// The replies to the calls of a `Step::Calls`, in the order of the calls.
    fn step_many(&mut self, _replies: Vec<Reply>) -> Result<Step<Self::Output>> {
        bail!("this request does not send calls side by side")
    }
}

/// Runs a flow with the desktop HTTP client until it is done. All its calls share one client, so a
/// connection (and its TLS session) is reused from call to call. A call that gets no reply ends
/// the flow with that error.
#[cfg(feature = "cloud")]
pub fn run<F: Flow>(flow: F) -> Result<F::Output> {
    drive(flow, |reply| reply)
}

/// Runs a flow like `run`, but a call that gets no reply at all (the server is not reached, the
/// connection drops) is answered with status 0, as the browser's `fetch` answers it. A flow that
/// can do without that reply (a second call that only words the result, one of two calls side by
/// side) then ends on the desktop as it does in the browser; `failure` words status 0 as
/// "Locora Cloud nicht erreichbar" where it cannot.
#[cfg(feature = "cloud")]
pub fn run_answering<F: Flow>(flow: F) -> Result<F::Output> {
    drive(flow, |reply| {
        Ok(reply.unwrap_or_else(|_| Reply {
            status: 0,
            body: Vec::new(),
        }))
    })
}

/// The loop of `run` and `run_answering`; `answer` decides what a call without a reply becomes.
#[cfg(feature = "cloud")]
fn drive<F: Flow>(mut flow: F, answer: fn(Result<Reply>) -> Result<Reply>) -> Result<F::Output> {
    let agent = native::agent();
    let mut step = flow.step(None)?;
    loop {
        step = match step {
            Step::Done(output) => return Ok(output),
            Step::Call(call) => flow.step(Some(answer(native::perform(&agent, &call))?))?,
            Step::Calls(calls) => flow.step_many(
                native::perform_all(&agent, &calls)
                    .into_iter()
                    .map(answer)
                    .collect::<Result<_>>()?,
            )?,
        };
    }
}

/// A reply that was not a success: its status and the server's message (the error's text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpError {
    /// 0 when no reply came at all.
    pub status: u16,
    pub message: String,
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for HttpError {}

/// The HTTP status behind an error, if it came from a reply (0: the server was not reached).
pub fn status_of(error: &anyhow::Error) -> Option<u16> {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<HttpError>())
        .map(|error| error.status)
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
    // Status 0 is a reply that never came: the page could not reach the server.
    // Full storage, too many requests and a busy server get the app's own words, which it
    // translates. They stay neutral: the apps never send anyone to a shop. Full storage of an
    // account whose address is not confirmed yet (a 507 with that reason) says what helps.
    let message = match reply.status {
        0 => "Locora Cloud nicht erreichbar".to_owned(),
        429 => "Gerade zu viele Anfragen an Locora Cloud. Bitte später erneut versuchen".to_owned(),
        503 => "Locora Cloud ist gerade ausgelastet. Bitte später erneut versuchen".to_owned(),
        507 if serde_json::from_slice::<ErrorBody>(&reply.body)
            .is_ok_and(|body| body.reason.as_deref() == Some(REASON_CONFIRM_EMAIL)) =>
        {
            "Der Speicher in Locora Cloud ist voll. Bestätige deine E-Mail-Adresse, dann gibt es mehr Platz".to_owned()
        }
        507 => "Der Speicher in Locora Cloud ist voll. Lösche nicht mehr benötigte Bilder oder Videos oder versuche es später erneut".to_owned(),
        _ => match serde_json::from_slice::<ErrorBody>(&reply.body) {
            Ok(body) => body.error,
            Err(_) => format!("Locora Cloud antwortet mit {}", reply.status),
        },
    };
    anyhow::Error::new(HttpError {
        status: reply.status,
        message,
    })
}

/// Whether a reply says the server does not know this request: an older server without the
/// endpoint (404, or 405 where the path matches another route).
pub fn unsupported(reply: &Reply) -> bool {
    matches!(reply.status, 404 | 405)
}

/// Whether a reply says the server has no such route, on a route whose newer server answers 404
/// itself for something it does not find (and then names it): 405, or 404 with the server's
/// general "Nicht gefunden" or with no error message at all (older servers still).
pub fn no_route(reply: &Reply) -> bool {
    match reply.status {
        405 => true,
        404 => serde_json::from_slice::<ErrorBody>(&reply.body)
            .map_or(true, |body| body.error == "Nicht gefunden"),
        _ => false,
    }
}

/// A single request whose reply is read by `read`.
pub struct Exchange<T> {
    call: Option<Call>,
    read: fn(Reply) -> Result<T>,
}

impl<T> Exchange<T> {
    pub fn new(call: Call, read: fn(Reply) -> Result<T>) -> Self {
        Self {
            call: Some(call),
            read,
        }
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
    if reply.is_success() {
        Ok(())
    } else {
        Err(failure(&reply))
    }
}

#[cfg(feature = "cloud")]
mod native {
    use super::*;
    use std::time::Duration;

    /// One client for all calls of a flow: it keeps connections open between calls.
    pub fn agent() -> ureq::Agent {
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .max_idle_connections_per_host(PARALLEL)
            .build()
            .into()
    }

    /// Sends calls side by side, one thread each; the replies (or why there is none) come in the
    /// order of the calls.
    pub fn perform_all(agent: &ureq::Agent, calls: &[Call]) -> Vec<Result<Reply>> {
        std::thread::scope(|scope| {
            let running: Vec<_> = calls
                .iter()
                .map(|call| scope.spawn(move || perform(agent, call)))
                .collect();
            running
                .into_iter()
                .map(|thread| {
                    thread
                        .join()
                        .map_err(|_| anyhow::anyhow!("a cloud request stopped unexpectedly"))?
                })
                .collect()
        })
    }

    /// Sends one call with ureq. A non-2xx status is a reply like any other.
    pub fn perform(agent: &ureq::Agent, call: &Call) -> Result<Reply> {
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
        let request = agent
            .configure_request(request)
            .timeout_global(Some(Duration::from_secs(call.timeout_secs)))
            .build();
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
        let ok: serde_json::Value = json(Reply {
            status: 200,
            body: br#"{"a":1}"#.to_vec(),
        })
        .unwrap();
        assert_eq!(ok["a"], 1);
        let empty: Option<u8> = json(Reply {
            status: 204,
            body: Vec::new(),
        })
        .unwrap();
        assert_eq!(empty, None);
        let failed = json::<serde_json::Value>(Reply {
            status: 403,
            body: br#"{"error":"no access"}"#.to_vec(),
        });
        assert_eq!(failed.unwrap_err().to_string(), "no access");
        let plain = json::<serde_json::Value>(Reply {
            status: 500,
            body: b"oops".to_vec(),
        });
        assert_eq!(
            plain.unwrap_err().to_string(),
            "Locora Cloud antwortet mit 500"
        );
        // Full storage, rate limits and a busy server get the app's own neutral words, even
        // where the server's message differs.
        for (status, start) in [
            (429, "Gerade zu viele Anfragen"),
            (503, "Locora Cloud ist gerade ausgelastet"),
            (507, "Der Speicher in Locora Cloud ist voll"),
        ] {
            let error = failure(&Reply {
                status,
                body: br#"{"error":"the server's own words"}"#.to_vec(),
            });
            assert!(error.to_string().starts_with(start), "{status}: {error}");
            assert_eq!(status_of(&error), Some(status));
        }
        // An account whose address is not confirmed has used its smaller storage: the app's own
        // words say that confirming the address helps, whatever the server's message is.
        let unconfirmed = failure(&Reply {
            status: 507,
            body: br#"{"error":"the server's own words","reason":"confirm_email"}"#.to_vec(),
        });
        assert_eq!(
            unconfirmed.to_string(),
            "Der Speicher in Locora Cloud ist voll. Bestätige deine E-Mail-Adresse, dann gibt es mehr Platz"
        );
        assert_eq!(status_of(&unconfirmed), Some(507));
        // Any other reason, or one on another status, changes nothing.
        let other = failure(&Reply {
            status: 507,
            body: br#"{"error":"voll","reason":"something_else"}"#.to_vec(),
        });
        assert!(
            other
                .to_string()
                .starts_with("Der Speicher in Locora Cloud ist voll. Lösche")
        );
        let not_full = failure(&Reply {
            status: 409,
            body: br#"{"error":"schon da","reason":"confirm_email"}"#.to_vec(),
        });
        assert_eq!(not_full.to_string(), "schon da");
    }

    #[test]
    fn a_missing_route_is_told_from_a_missing_thing() {
        let reply = |status: u16, body: &str| Reply {
            status,
            body: body.as_bytes().to_vec(),
        };
        assert!(no_route(&reply(
            405,
            r#"{"error":"Diese Methode ist hier nicht erlaubt"}"#
        )));
        assert!(no_route(&reply(405, "")));
        assert!(no_route(&reply(404, r#"{"error":"Nicht gefunden"}"#)));
        assert!(no_route(&reply(404, "")));
        assert!(no_route(&reply(404, "Not Found")));
        // A newer server names what it did not find.
        assert!(!no_route(&reply(
            404,
            r#"{"error":"Diese Einladung gibt es nicht"}"#
        )));
        assert!(!no_route(&reply(410, "")));
        assert!(!no_route(&reply(204, "")));
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

    /// A call, two calls side by side, one more call; it ends with the statuses it got.
    #[cfg(feature = "cloud")]
    struct Dropped {
        server: String,
        statuses: Vec<u16>,
    }

    #[cfg(feature = "cloud")]
    impl Dropped {
        fn get(&self, path: &str) -> Call {
            Call {
                method: Method::Get,
                url: format!("{}{path}", self.server),
                token: None,
                body: None,
                timeout_secs: 5,
            }
        }
    }

    #[cfg(feature = "cloud")]
    impl Flow for Dropped {
        type Output = Vec<u16>;

        fn step(&mut self, reply: Option<Reply>) -> Result<Step<Vec<u16>>> {
            match reply {
                None => Ok(Step::Call(self.get("/first"))),
                Some(reply) => {
                    self.statuses.push(reply.status);
                    if self.statuses.len() == 1 {
                        Ok(Step::Calls(vec![self.get("/kept"), self.get("/dropped")]))
                    } else {
                        Ok(Step::Done(std::mem::take(&mut self.statuses)))
                    }
                }
            }
        }

        fn step_many(&mut self, replies: Vec<Reply>) -> Result<Step<Vec<u16>>> {
            self.statuses
                .extend(replies.iter().map(|reply| reply.status));
            Ok(Step::Call(self.get("/dropped")))
        }
    }

    /// A server on a local port that answers every path with 200, except `/dropped`: there the
    /// connection closes without a reply (a lost network, a server that went away).
    #[cfg(feature = "cloud")]
    fn dropping_server() -> String {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let server = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for mut socket in listener.incoming().flatten() {
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(&socket);
                    let mut request = String::new();
                    reader.read_line(&mut request).unwrap();
                    loop {
                        let mut line = String::new();
                        reader.read_line(&mut line).unwrap();
                        if line == "\r\n" || line.is_empty() {
                            break;
                        }
                    }
                    if !request.contains("/dropped") {
                        socket
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                            .unwrap();
                    }
                });
            }
        });
        server
    }

    #[cfg(feature = "cloud")]
    #[test]
    fn a_call_without_a_reply_is_status_0_where_the_flow_answers_it_like_the_browser() {
        let flow = |server: &str| Dropped {
            server: server.to_owned(),
            statuses: Vec::new(),
        };
        let server = dropping_server();
        // As the browser's fetch: one of two calls side by side and a later call get status 0,
        // and the flow decides what that means.
        assert_eq!(run_answering(flow(&server)).unwrap(), [200, 200, 0, 0]);
        // `run` ends the flow at the first call without a reply.
        let error = run(flow(&server)).unwrap_err();
        assert_eq!(error.to_string(), "Locora Cloud nicht erreichbar");
        // Status 0 is worded as a server out of reach.
        assert_eq!(
            failure(&Reply {
                status: 0,
                body: Vec::new()
            })
            .to_string(),
            "Locora Cloud nicht erreichbar"
        );
    }

    #[test]
    fn a_flow_asks_for_its_call_then_finishes_with_the_reply() {
        let mut flow = Echo;
        let call = match flow.step(None).unwrap() {
            Step::Call(call) => call,
            _ => panic!("the flow must ask first"),
        };
        assert_eq!(call.method.as_str(), "GET");
        assert!(matches!(
            flow.step(Some(Reply {
                status: 418,
                body: Vec::new()
            })),
            Ok(Step::Done(418))
        ));
    }
}
