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

// The protocol wire tags for a request (int-keyed per the wire IR — a rename never moves bytes). The client
// emits PLAIN DATA: it names a verb + operands, never an engine/host type. The daemon (the composition
// root) interprets these tags and runs the build; this crate stays inside CLIENT_CLOSURE (G7).
/// Field tag 0: the verb string (e.g. `"build"`, `"ping"`).
pub const REQ_VERB: i64 = 0;
/// Field tag 1: the build verb's target-pattern operand (e.g. `"//hello:out.txt"`).
pub const REQ_TARGET_PATTERN: i64 = 1;

/// Parse an argv surface into a protocol `Request`. `ping [text…]` → Text IR (skeleton). `build <pattern>`
/// → an int-keyed `Map` request carrying the verb + the single target pattern — engine-blind DATA the
/// daemon interprets. Fail-closed: `build` with no pattern, or with extra args, is an `UnknownCommand`-class
/// usage error (v1 builds exactly one pattern).
pub fn parse_request(args: &[String]) -> Result<Request, CliError> {
    match args.split_first() {
        None => Ok(Request(WireValue::Null)),
        Some((verb, rest)) if verb == "ping" => Ok(Request(WireValue::Text(rest.join(" ")))),
        Some((verb, rest)) if verb == "build" => match rest {
            [pattern] => Ok(Request(WireValue::Map(vec![
                (REQ_VERB, WireValue::Text("build".to_string())),
                (REQ_TARGET_PATTERN, WireValue::Text(pattern.clone())),
            ]))),
            _ => Err(CliError::UnknownCommand(format!("build expects exactly one target pattern, got {}", rest.len()))),
        },
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
            run_call(&t, &["frobnicate".into()]),
            Err(CliError::UnknownCommand("frobnicate".into()))
        );
    }

    #[test]
    fn build_parses_to_engine_blind_data() {
        // The client emits PLAIN wire DATA for `build <pattern>` — a verb + a pattern operand, no engine
        // type in sight (the whole crate stays within CLIENT_CLOSURE). The daemon interprets these tags.
        let req = parse_request(&["build".into(), "//hello:out.txt".into()]).expect("build parses");
        assert_eq!(
            req.0,
            WireValue::Map(vec![
                (REQ_VERB, WireValue::Text("build".into())),
                (REQ_TARGET_PATTERN, WireValue::Text("//hello:out.txt".into())),
            ])
        );
    }

    #[test]
    fn build_without_pattern_fails_closed() {
        let t = echo();
        assert!(matches!(run_call(&t, &["build".into()]), Err(CliError::UnknownCommand(_))),
            "build with no target pattern is a fail-closed usage error");
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
