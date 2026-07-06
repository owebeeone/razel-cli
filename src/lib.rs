//! `razel-cli` — the argument surface + rendering, a PURE protocol client (ADR-0006; gate G7). Role
//! `client` in the dependency-deny wall: this crate may link ONLY the pinned client closure
//! {razel-comms, razel-wire-api, razel-wire-cbor} — it can never NAME an engine type, so an in-process
//! build fails to LINK, not merely fails review. The shipped `razel` multi-call binary is razel-daemon
//! linking THIS crate as a library and handing it an in-memory `razel_comms::Transport`.
//!
//! SKELETON: one `ping` verb over the IR; the real argv surface lands with the protocol schema
//! (DR55 C14). The load-bearing part is the SHAPE: parse → send over the seam → render, transport-blind.

use razel_comms::{CommsError, Event, Request, Transport};
use razel_wire_api::WireValue;
use razel_wire_cbor::CborCodec;

/// Fail-closed client errors — typed, never a silent default.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CliError {
    Comms(CommsError),
    UnknownCommand(String),
}

/// The one codec the standalone client speaks (§5 item 6: the codec impl is INSIDE the closure —
/// a client binary must be able to encode without a composition root).
pub fn default_codec() -> CborCodec {
    CborCodec
}

/// Parse an argv surface into a protocol `Request`. SKELETON: `ping [text…]` → Text IR.
pub fn parse_request(args: &[String]) -> Result<Request, CliError> {
    match args.split_first() {
        None => Ok(Request(WireValue::Null)),
        Some((verb, rest)) if verb == "ping" => Ok(Request(WireValue::Text(rest.join(" ")))),
        Some((verb, _)) => Err(CliError::UnknownCommand(verb.clone())),
    }
}

/// Render one event to a display line. SKELETON rendering over the IR.
pub fn render_event(ev: &Event) -> String {
    match &ev.0 {
        WireValue::Text(s) => s.clone(),
        WireValue::Int(i) => i.to_string(),
        WireValue::Bool(b) => b.to_string(),
        WireValue::Null => "()".to_string(),
        other => format!("{other:?}"),
    }
}

/// ONE client call over the transport seam: parse → send → render. This is the multi-call surface —
/// a composition root may invoke it any number of times over one live transport (the ratified
/// `--batch` embedded shape), or a socket client can drive it remotely; this crate cannot tell.
pub fn run_call(transport: &dyn Transport, args: &[String]) -> Result<Vec<String>, CliError> {
    let req = parse_request(args)?;
    let events = transport.send(req).map_err(CliError::Comms)?;
    Ok(events.iter().map(render_event).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use razel_comms::InMemoryTransport;

    fn echo() -> InMemoryTransport {
        InMemoryTransport::new(|req: Request| Ok(vec![Event(req.0)]))
    }

    #[test]
    fn ping_round_trips_over_the_seam() {
        let t = echo();
        let out = run_call(&t, &["ping".into(), "hello".into(), "wall".into()]).expect("call succeeds");
        assert_eq!(out, vec!["hello wall".to_string()]);
    }

    #[test]
    fn unknown_verb_fails_closed() {
        let t = echo();
        assert_eq!(
            run_call(&t, &["build".into()]),
            Err(CliError::UnknownCommand("build".into()))
        );
    }

    #[test]
    fn transport_failure_is_surfaced_typed() {
        let t = InMemoryTransport::new(|_| Err(CommsError::Closed));
        assert_eq!(run_call(&t, &[]), Err(CliError::Comms(CommsError::Closed)));
    }

    #[test]
    fn request_identity_via_the_closure_codec() {
        // The client CAN digest what it sends — using only closure crates (comms + wire-api + wire-cbor).
        let codec = default_codec();
        let a = parse_request(&["ping".into(), "x".into()]).unwrap();
        let b = parse_request(&["ping".into(), "x".into()]).unwrap();
        assert_eq!(a.digest(&codec), b.digest(&codec));
    }
}
