//! `razel-cli` — the argument surface + rendering, a PURE protocol client (ADR-0006; gate G7). Role
//! `client` in the wall: it may link ONLY the pinned closure {razel-comms, razel-wire-api, razel-wire-cbor}
//! — it can never NAME an engine type, so an in-process build fails to LINK. The shipped `razel` multi-call
//! binary is razel-daemon linking THIS crate as a library and handing it a `razel_comms::Transport`.
//!
//! The T10 hand-rolled request is GONE: `parse_request` emits a TYPED `razel_comms::Request` (the closed IR
//! inventory), and `render_event` is an EXHAUSTIVE match over the typed `Event` arms (no wildcard — feeds
//! `renderer_match_exhaustive_without_wildcard`). The load-bearing shape survives: parse → send over the
//! seam → render, transport-blind.

use razel_comms::{CommsError, Event, Request, Transport};
use razel_wire_api::protocol::generated as gen;
use razel_wire_cbor::CborCodec;

/// Fail-closed client errors — typed, never a silent default.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CliError {
    Comms(CommsError),
    UnknownCommand(String),
    Usage(String),
}

/// The one codec the standalone client speaks (ratified §5 item 6: a client binary must pick a codec from
/// inside the closure — no composition root needed to encode).
pub fn default_codec() -> CborCodec {
    CborCodec
}

/// Parse an argv surface into a typed protocol `Request` (the closed IR inventory). v1 verbs: `build
/// <pattern>` (exactly one), `ping`, `version`. The build request carries the pattern in `args`; the
/// invocation id is left empty (the server is authoritative). Fail-closed: an unknown verb or a `build`
/// with the wrong arity is a typed usage error, never a guess.
pub fn parse_request(args: &[String]) -> Result<Request, CliError> {
    match args.split_first() {
        None => Ok(Request::Ping),
        Some((verb, _)) if verb == "ping" => Ok(Request::Ping),
        Some((verb, _)) if verb == "version" => Ok(Request::Version),
        Some((verb, rest)) if verb == "build" => match rest {
            [pattern] => Ok(Request::Build(gen::BuildRequest {
                invocation_id: String::new(),
                args: vec![pattern.clone()],
                cwd: String::new(),
            })),
            _ => Err(CliError::Usage(format!("build expects exactly one target pattern, got {}", rest.len()))),
        },
        Some((verb, _)) => Err(CliError::UnknownCommand(verb.clone())),
    }
}

/// Render one event to zero-or-more display lines. EXHAUSTIVE over the typed `Event` arms (no wildcard):
/// adding an IR event kind forces a compile error here.
pub fn render_event(ev: &Event) -> Vec<String> {
    match ev {
        Event::Welcome(w) => vec![format!("welcome (protocol {}, server {})", w.protocol, w.server_version)],
        Event::SubmitAck(a) => vec![format!("submitted {}", a.invocation_id)],
        Event::Stream(e) => render_stream(e),
        Event::Pong(_) => vec!["pong".to_string()],
        Event::VersionInfo(v) => vec![format!("razel {} (protocol {})", v.server_version, v.protocol)],
        Event::CancelAck(c) => vec![format!("cancel {} accepted={}", c.invocation_id, c.accepted)],
        Event::ShutdownAck(s) => vec![format!("shutdown draining={}", s.draining)],
        Event::Impact(i) => i.affected.iter().map(|f| format!("affected {f}")).collect(),
        Event::Error(e) => vec![format!("error: {:?}: {}", e.code, e.detail.clone().unwrap_or_default())],
    }
}

/// Render one streaming `events.subscribe` record. EXHAUSTIVE over the IR `EventKind` (no wildcard).
/// Non-display kinds (accepted/progress) render to no lines; the terminal `result` renders one line per
/// built output — the T10 headline shape.
fn render_stream(e: &gen::Event) -> Vec<String> {
    match e.kind {
        gen::EventKind::Accepted => Vec::new(),
        gen::EventKind::Progress => Vec::new(),
        gen::EventKind::Diagnostic => e
            .diag
            .as_ref()
            .map(|d| vec![format!("diag: {:?}: {}", d.code, d.detail.clone().unwrap_or_default())])
            .unwrap_or_default(),
        gen::EventKind::Result => e
            .result
            .as_ref()
            .map(|r| r.outputs.iter().map(|o| format!("built {} -> {}", o.exec_path, o.host_path)).collect())
            .unwrap_or_default(),
        gen::EventKind::Error => e
            .error
            .as_ref()
            .map(|er| vec![format!("error: {:?}: {}", er.code, er.detail.clone().unwrap_or_default())])
            .unwrap_or_default(),
        gen::EventKind::Cancelled => vec!["cancelled".to_string()],
    }
}

/// ONE client call over the transport seam: parse → send → render. The multi-call surface — a composition
/// root may invoke it any number of times over one live transport (the `--batch` embedded shape), or a
/// socket client can drive it remotely; this crate cannot tell.
pub fn run_call(transport: &dyn Transport, args: &[String]) -> Result<Vec<String>, CliError> {
    let req = parse_request(args)?;
    let events = transport.send(req).map_err(CliError::Comms)?;
    Ok(events.iter().flat_map(render_event).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use razel_comms::{InMemoryTransport, LogReadRequest, LogReadResponse, Reply, Server};
    use std::sync::Arc;

    /// A tiny in-process server for the CLI unit tests: echoes typed acks, and serves a one-shot build log
    /// (Accepted + Result) so the render path is exercised without the engine.
    struct StubServer;
    impl Server for StubServer {
        fn dispatch(&self, req: Request) -> Result<Reply, CommsError> {
            Ok(match req {
                Request::Ping => Reply::Ack(Event::Pong(gen::Pong { token: None })),
                Request::Version => Reply::Ack(Event::VersionInfo(gen::VersionInfo {
                    server_version: "stub".into(),
                    protocol: 1,
                })),
                Request::Build(_) => {
                    // No log registry needed: a stub streaming reply the transport will drain — but the
                    // in-memory transport drains via read_log, so return an Ack error is simpler for a stub.
                    Reply::Ack(Event::Stream(gen::Event {
                        kind: gen::EventKind::Result,
                        result: Some(gen::ResultPayload {
                            success: true,
                            outputs: vec![gen::BuiltOutput { exec_path: "a/b".into(), host_path: "/w/a/b".into() }],
                        }),
                        ..Default::default()
                    }))
                }
                other => Reply::Ack(Event::Error(gen::ProtocolError {
                    code: gen::ProtocolErrorCode::Unsupported,
                    detail: Some(format!("stub: {}", other.method_name())),
                })),
            })
        }
        fn read_log(&self, _inv: &str, _req: LogReadRequest) -> Result<LogReadResponse, CommsError> {
            Ok(LogReadResponse { records: vec![], next_cursor: 0, state: razel_comms::LogReadState::Eof, error: None })
        }
    }

    fn stub() -> InMemoryTransport {
        InMemoryTransport::new(Arc::new(StubServer))
    }

    #[test]
    fn ping_round_trips_over_the_seam() {
        let out = run_call(&stub(), &["ping".into()]).expect("call succeeds");
        assert_eq!(out, vec!["pong".to_string()]);
    }

    #[test]
    fn version_renders_typed() {
        let out = run_call(&stub(), &["version".into()]).expect("call succeeds");
        assert_eq!(out, vec!["razel stub (protocol 1)".to_string()]);
    }

    #[test]
    fn unknown_verb_fails_closed() {
        assert_eq!(run_call(&stub(), &["frobnicate".into()]), Err(CliError::UnknownCommand("frobnicate".into())));
    }

    #[test]
    fn build_parses_to_typed_request() {
        // The client emits a TYPED Request::Build carrying the pattern in args — no hand-rolled WireValue::Map.
        let req = parse_request(&["build".into(), "//hello:out.txt".into()]).expect("build parses");
        match req {
            Request::Build(b) => {
                assert_eq!(b.args, vec!["//hello:out.txt".to_string()]);
                assert_eq!(b.invocation_id, "", "the server is authoritative for the invocation id");
            }
            other => panic!("expected Request::Build, got {other:?}"),
        }
    }

    #[test]
    fn build_result_renders_one_line_per_output() {
        let out = run_call(&stub(), &["build".into(), "//a:b".into()]).expect("build call");
        assert_eq!(out, vec!["built a/b -> /w/a/b".to_string()]);
    }

    #[test]
    fn build_without_pattern_fails_closed() {
        assert!(matches!(parse_request(&["build".into()]), Err(CliError::Usage(_))),
            "build with no target pattern is a fail-closed usage error");
    }

    #[test]
    fn transport_failure_is_surfaced_typed() {
        struct DeadServer;
        impl Server for DeadServer {
            fn dispatch(&self, _req: Request) -> Result<Reply, CommsError> {
                Err(CommsError::Closed)
            }
            fn read_log(&self, _i: &str, _r: LogReadRequest) -> Result<LogReadResponse, CommsError> {
                Err(CommsError::Closed)
            }
        }
        let t = InMemoryTransport::new(Arc::new(DeadServer));
        assert_eq!(run_call(&t, &["ping".into()]), Err(CliError::Comms(CommsError::Closed)));
    }

    #[test]
    fn client_can_pick_a_codec_from_the_closure() {
        // The standalone client can name a codec (razel-wire-cbor) without a composition root (ratified §5.6).
        let _codec = default_codec();
    }
}
