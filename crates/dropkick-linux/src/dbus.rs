//! A minimal D-Bus client: enough to make a handful of calls, and no more.
//!
//! ## Why not a D-Bus crate
//!
//! Dropkick needs three things from D-Bus — a desktop notification for the
//! Timer, `PickColor` for the Color eyedropper, and a secret for encrypting
//! pinned clipboard entries. That is three method calls, not a library.
//!
//! `zbus` is the mainstream choice and would work; it brings an async runtime
//! and dozens of crates. The `dbus` crate wraps libdbus, adding a C dependency
//! and a system package. Neither threatens the no-networking rule — D-Bus is an
//! `AF_UNIX` socket, and the planned seccomp filter blocks `AF_INET` only — so
//! this was a question of weight and auditability, not security.
//!
//! Hand-rolling wins on one specific point: with the `dbus` cargo feature off,
//! a Dropkick binary contains *no D-Bus code at all*, which is checkable by
//! looking rather than by reading a dependency tree.
//!
//! The honest cost is that the marshalling below is ours to get right.
//! Alignment and padding are where D-Bus implementations go wrong, so the
//! encoder is tested against byte layouts taken from the specification.
//!
//! ## What is deliberately missing
//!
//! No signal handling, no properties, no introspection, no object server, no
//! fd passing. `PickColor` and the secret portal need the Request/Response
//! signal pattern and will extend this; they are not here yet.

use std::io::{self, Read, Write};
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;

const LITTLE_ENDIAN: u8 = b'l';
const MSG_METHOD_CALL: u8 = 1;
const MSG_METHOD_RETURN: u8 = 2;
const MSG_ERROR: u8 = 3;
const MSG_SIGNAL: u8 = 4;

// Header field codes, from the specification.
const FIELD_PATH: u8 = 1;
const FIELD_INTERFACE: u8 = 2;
const FIELD_MEMBER: u8 = 3;
const FIELD_ERROR_NAME: u8 = 4;
const FIELD_DESTINATION: u8 = 6;
const FIELD_SIGNATURE: u8 = 8;

/// Little-endian D-Bus encoder.
///
/// Every write aligns first: D-Bus pads each value to its own alignment,
/// measured from the start of the message body, and getting that wrong
/// produces messages a bus silently rejects.
#[derive(Default)]
pub struct Encoder {
    buf: Vec<u8>,
}

impl Encoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    fn align(&mut self, to: usize) {
        while !self.buf.len().is_multiple_of(to) {
            self.buf.push(0);
        }
    }

    pub fn byte(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub fn u32(&mut self, v: u32) {
        self.align(4);
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn i32(&mut self, v: i32) {
        self.align(4);
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    /// STRING and OBJECT_PATH: 4-byte length, bytes, NUL terminator.
    pub fn string(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.buf.extend_from_slice(s.as_bytes());
        self.buf.push(0);
    }

    /// SIGNATURE: single-byte length, so it is *not* 4-aligned.
    pub fn signature(&mut self, s: &str) {
        self.buf.push(s.len() as u8);
        self.buf.extend_from_slice(s.as_bytes());
        self.buf.push(0);
    }

    /// A variant is its own signature followed by the value.
    pub fn variant_string(&mut self, s: &str) {
        self.signature("s");
        self.string(s);
    }

    pub fn variant_byte(&mut self, v: u8) {
        self.signature("y");
        self.byte(v);
    }

    /// One `{sv}` pair. Dict entries align to 8, same as structs.
    pub fn dict_entry_string(&mut self, key: &str, value: &str) {
        self.structure(|e| {
            e.string(key);
            e.variant_string(value);
        });
    }

    pub fn dict_entry_byte(&mut self, key: &str, value: u8) {
        self.structure(|e| {
            e.string(key);
            e.variant_byte(value);
        });
    }

    /// ARRAY: a 4-byte byte-count, then padding to the *element's* alignment,
    /// then the elements. The padding is required even when the array is
    /// empty, which is the single most common D-Bus encoding bug.
    pub fn array<F: FnOnce(&mut Encoder)>(&mut self, element_align: usize, f: F) {
        self.align(4);
        let len_at = self.buf.len();
        self.buf.extend_from_slice(&0u32.to_le_bytes());
        self.align(element_align);
        let body_start = self.buf.len();
        f(self);
        let body_len = (self.buf.len() - body_start) as u32;
        self.buf[len_at..len_at + 4].copy_from_slice(&body_len.to_le_bytes());
    }

    /// STRUCT and DICT_ENTRY both align to 8.
    pub fn structure<F: FnOnce(&mut Encoder)>(&mut self, f: F) {
        self.align(8);
        f(self);
    }
}

fn read_f64_le(b: &[u8]) -> Option<f64> {
    Some(f64::from_le_bytes(b.get(..8)?.try_into().ok()?))
}

fn read_u32_le(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// A decoded message header plus its raw body.
struct Message {
    msg_type: u8,
    reply_serial: u32,
    body: Vec<u8>,
    error_name: Option<String>,
    /// The member name, which is how a signal is recognised.
    member: Option<String>,
}

/// How hard a notification should try to be noticed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alert {
    /// Normal urgency, server-chosen timeout, no sound.
    Info,
    /// Critical urgency, stays until dismissed. `sound` asks the server to
    /// play the alarm sound too, which is a setting because whether an alert
    /// should make noise is not something software gets to decide for someone.
    Alarm { sound: bool },
}

pub struct Bus {
    stream: UnixStream,
    serial: u32,
    /// The name the bus assigned us, like `:1.207`. Discarded until the
    /// eyedropper needed it: the portal builds a Request's object path out of
    /// it, and the match rule has to name that path before the call is made.
    unique_name: String,
}

impl Bus {
    /// Connect to the session bus and complete the SASL handshake.
    pub fn session() -> io::Result<Bus> {
        let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS")
            .map_err(|_| io::Error::other("DBUS_SESSION_BUS_ADDRESS is not set"))?;
        let path = addr
            .split(',')
            .find_map(|part| part.strip_prefix("unix:path="))
            .ok_or_else(|| io::Error::other(format!("unsupported bus address: {addr}")))?;

        let mut bus =
            Bus { stream: UnixStream::connect(path)?, serial: 0, unique_name: String::new() };
        bus.authenticate()?;
        bus.hello()?;
        Ok(bus)
    }

    /// EXTERNAL auth: the kernel already told the bus who we are over the
    /// socket, so the credential is just our uid, hex-encoded.
    fn authenticate(&mut self) -> io::Result<()> {
        // The leading NUL byte is required before any SASL traffic.
        self.stream.write_all(&[0])?;

        let uid = std::fs::metadata("/proc/self")?.uid();
        let hex: String = uid.to_string().bytes().map(|b| format!("{b:02x}")).collect();
        self.stream.write_all(format!("AUTH EXTERNAL {hex}\r\n").as_bytes())?;

        let reply = self.read_line()?;
        if !reply.starts_with("OK") {
            return Err(io::Error::other(format!("bus rejected auth: {reply}")));
        }
        self.stream.write_all(b"BEGIN\r\n")?;
        Ok(())
    }

    fn read_line(&mut self) -> io::Result<String> {
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            self.stream.read_exact(&mut byte)?;
            if byte[0] == b'\n' {
                break;
            }
            if byte[0] != b'\r' {
                line.push(byte[0]);
            }
            if line.len() > 512 {
                return Err(io::Error::other("auth line too long"));
            }
        }
        Ok(String::from_utf8_lossy(&line).into_owned())
    }

    /// Every client must say Hello before the bus will route anything else.
    fn hello(&mut self) -> io::Result<()> {
        let reply = self.call(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "Hello",
            "",
            Vec::new(),
        )?;
        // The reply body is a single string: our unique name.
        if reply.len() >= 4 {
            let len = read_u32_le(&reply[..4]) as usize;
            if reply.len() >= 4 + len {
                self.unique_name = String::from_utf8_lossy(&reply[4..4 + len]).into_owned();
            }
        }
        Ok(())
    }

    /// Send a method call and wait for its reply.
    pub fn call(
        &mut self,
        destination: &str,
        path: &str,
        interface: &str,
        member: &str,
        signature: &str,
        body: Vec<u8>,
    ) -> io::Result<Vec<u8>> {
        self.serial += 1;
        let serial = self.serial;

        let mut msg = Encoder::new();
        msg.byte(LITTLE_ENDIAN);
        msg.byte(MSG_METHOD_CALL);
        msg.byte(0); // flags
        msg.byte(1); // protocol version
        msg.u32(body.len() as u32);
        msg.u32(serial);

        msg.array(8, |fields| {
            let mut field = |code: u8, sig: &str, value: &str| {
                fields.structure(|f| {
                    f.byte(code);
                    f.signature(sig);
                    f.string(value);
                });
            };
            field(FIELD_PATH, "o", path);
            field(FIELD_INTERFACE, "s", interface);
            field(FIELD_MEMBER, "s", member);
            field(FIELD_DESTINATION, "s", destination);
            if !signature.is_empty() {
                fields.structure(|f| {
                    f.byte(FIELD_SIGNATURE);
                    f.signature("g");
                    f.signature(signature);
                });
            }
        });

        // The body always starts on an 8-byte boundary.
        let mut out = msg.into_bytes();
        while !out.len().is_multiple_of(8) {
            out.push(0);
        }
        out.extend_from_slice(&body);
        self.stream.write_all(&out)?;

        self.await_reply(serial)
    }

    fn await_reply(&mut self, serial: u32) -> io::Result<Vec<u8>> {
        // Other traffic (signals, replies to Hello) can arrive first.
        for _ in 0..32 {
            let Message { msg_type, reply_serial, body, error_name: error, .. } =
                self.read_message()?;
            if msg_type == MSG_ERROR && reply_serial == serial {
                return Err(io::Error::other(format!(
                    "{}: {}",
                    error.unwrap_or_else(|| "d-bus error".into()),
                    String::from_utf8_lossy(&body)
                        .chars()
                        .filter(|c| !c.is_control())
                        .collect::<String>()
                )));
            }
            if msg_type == MSG_METHOD_RETURN && reply_serial == serial {
                return Ok(body);
            }
        }
        Err(io::Error::other("no reply from the bus"))
    }

    /// One message off the bus.
    fn read_message(&mut self) -> io::Result<Message> {
        let mut fixed = [0u8; 16];
        self.stream.read_exact(&mut fixed)?;
        if fixed[0] != LITTLE_ENDIAN {
            return Err(io::Error::other("big-endian bus messages are not supported"));
        }
        let msg_type = fixed[1];
        let body_len = read_u32_le(&fixed[4..8]) as usize;
        let fields_len = read_u32_le(&fixed[12..16]) as usize;

        let padded_fields = fields_len.div_ceil(8) * 8;
        let mut rest = vec![0u8; padded_fields + body_len];
        self.stream.read_exact(&mut rest)?;

        let (reply_serial, error_name, member) = parse_header_fields(&rest[..fields_len]);
        let body = rest[padded_fields..].to_vec();
        Ok(Message { msg_type, reply_serial, body, error_name, member })
    }

    /// Ask the desktop portal for a colour picked off the screen.
    ///
    /// This is the first thing here that needs a **signal**, and it is why the
    /// eyedropper waited for the rest of the tools. `PickColor` does not return
    /// the colour; it returns an object path for a pending Request, and the
    /// answer arrives later as `org.freedesktop.portal.Request.Response` on
    /// that path -- after the user has clicked something, which may be never.
    ///
    /// Two consequences:
    ///
    /// * A match rule has to be installed **before** the call. Installing it
    ///   afterwards is a race the user would lose by picking quickly.
    /// * This blocks until the user picks or cancels, so it must not run on
    ///   the daemon's thread. See `eyedropper` in `main.rs`.
    ///
    /// Returns `Ok(None)` when the user cancelled, which is not an error.
    pub fn pick_color(&mut self) -> io::Result<PickResult> {
        // The portal derives the Request path from our bus name and a token, so
        // the rule can be installed before anything is sent.
        let token = "dropkick_pick";
        let sender = self.unique_name.trim_start_matches(':').replace('.', "_");
        let handle = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");

        let mut rule = Encoder::new();
        rule.string(&format!(
            "type='signal',interface='org.freedesktop.portal.Request',member='Response',path='{handle}'"
        ));
        self.call(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "AddMatch",
            "s",
            rule.into_bytes(),
        )?;

        let mut body = Encoder::new();
        body.string(""); // parent_window: we are a layer surface, so none
        body.array(8, |opts| {
            opts.dict_entry_string("handle_token", token);
        });
        self.call(
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Screenshot",
            "PickColor",
            "sa{sv}",
            body.into_bytes(),
        )?;

        // Wait for the Response, but not forever. The user is being asked to
        // click something and may take a while, so there is no message-count
        // limit like `await_reply` has -- but without a deadline a portal that
        // never answers parks this thread for the life of the daemon, holding
        // the panel hidden with nothing to explain it.
        self.stream.set_read_timeout(Some(std::time::Duration::from_secs(120)))?;
        loop {
            let msg = match self.read_message() {
                Ok(m) => m,
                Err(e)
                    if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) =>
                {
                    return Ok(PickResult::TimedOut);
                }
                Err(e) => return Err(e),
            };
            if msg.msg_type != MSG_SIGNAL || msg.member.as_deref() != Some("Response") {
                if std::env::var_os("DROPKICK_DEBUG").is_some() {
                    eprintln!("  [pick] ignoring type {} member {:?}", msg.msg_type, msg.member);
                }
                continue;
            }
            if std::env::var_os("DROPKICK_DEBUG").is_some() {
                eprintln!("  [pick] response body: {} bytes", msg.body.len());
            }
            return Ok(parse_pick_response(&msg.body));
        }
    }

    /// Post a desktop notification. Returns the server's notification id.
    ///
    /// The default notification is a box in the corner that vanishes in a few
    /// seconds, which is fine for "copied" and useless for "your timer went
    /// off". [`Alert::Alarm`] asks for the three things that change that, all
    /// of which this machine's server advertises: critical urgency, no
    /// auto-dismiss, and a sound.
    pub fn notify(&mut self, summary: &str, body_text: &str, alert: Alert) -> io::Result<u32> {
        let mut b = Encoder::new();
        b.string("Dropkick"); // app_name
        b.u32(0); // replaces_id
        b.string(""); // app_icon
        b.string(summary);
        b.string(body_text);
        b.array(4, |_| {}); // actions: as, empty
        b.array(8, |hints| match alert {
            Alert::Info => {}
            Alert::Alarm { sound } => {
                // 2 = critical. Servers keep critical notifications on screen
                // rather than expiring them.
                hints.dict_entry_byte("urgency", 2);
                if sound {
                    // From the XDG sound naming spec; the name for exactly this.
                    hints.dict_entry_string("sound-name", "alarm-clock-elapsed");
                }
            }
        });
        // 0 means "never expire on your own"; -1 would mean "server decides",
        // and the server decides on a few seconds.
        b.i32(match alert {
            Alert::Info => -1,
            Alert::Alarm { .. } => 0,
        });

        let reply = self.call(
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "Notify",
            "susssasa{sv}i",
            b.into_bytes(),
        )?;
        if reply.len() < 4 {
            return Err(io::Error::other("short reply to Notify"));
        }
        Ok(read_u32_le(&reply[..4]))
    }
}

/// Pull REPLY_SERIAL and ERROR_NAME out of a header field array.
/// Decode `(u response, a{sv} results)` far enough to find `color`, which is a
/// `(ddd)` of red, green and blue in 0..=1.
///
/// Hand-rolled and deliberately narrow: a general variant decoder is a lot of
/// surface for one message shape. It walks to the doubles rather than parsing
/// the dictionary properly, and returns `None` on anything unexpected.
pub fn parse_pick_response(body: &[u8]) -> PickResult {
    if body.len() < 4 {
        return PickResult::Undecodable;
    }
    // 0 = success. 1 = the user cancelled, 2 = something else ended it; both
    // are "no colour", not failures to report.
    let response = read_u32_le(&body[..4]);
    if response != 0 {
        return PickResult::Cancelled(response);
    }
    // Find the "color" key, then the three doubles that follow it. Doubles are
    // 8-aligned, so scan forward to the first alignment boundary that yields
    // three plausible values.
    let key = b"color";
    let Some(found) = body.windows(key.len()).position(|w| w == key) else {
        return PickResult::Undecodable;
    };
    let at = found + key.len();
    // Inclusive of the last offset that still has 24 bytes after it. An
    // exclusive bound here misses the common case, where the doubles are the
    // final thing in the body.
    let mut start = at.next_multiple_of(8);
    while start + 24 <= body.len() {
        let (Some(r), Some(g), Some(b)) = (
            read_f64_le(&body[start..]),
            read_f64_le(&body[start + 8..]),
            read_f64_le(&body[start + 16..]),
        ) else {
            break;
        };
        // Colour components are 0..=1. Anything else means the scan landed on
        // padding or on the signature rather than the values.
        let ok = |v: f64| v.is_finite() && (0.0..=1.0).contains(&v);
        if ok(r) && ok(g) && ok(b) {
            return PickResult::Picked(r, g, b);
        }
        start += 8;
    }
    // Success was reported but the payload was not what was expected. This
    // used to be indistinguishable from a cancellation, which meant a decoding
    // bug looked exactly like the user changing their mind.
    PickResult::Undecodable
}

/// What came back from a colour pick. Cancellation and "could not read the
/// reply" are deliberately different: reporting a decode failure as a
/// cancellation hides the bug and tells the user nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PickResult {
    Picked(f64, f64, f64),
    /// The portal said no. Carries the response code: 1 is the user
    /// cancelling, anything else is the portal ending it for its own reasons.
    Cancelled(u32),
    Undecodable,
    /// The portal never answered. Distinct from every other case: it means the
    /// request was accepted and then nothing came back.
    TimedOut,
}

fn parse_header_fields(mut fields: &[u8]) -> (u32, Option<String>, Option<String>) {
    const FIELD_REPLY_SERIAL: u8 = 5;
    let mut reply_serial = 0u32;
    let mut error_name = None;
    let mut member = None;
    let start = fields.as_ptr() as usize;

    while fields.len() >= 4 {
        // Each field is a struct, aligned to 8 from the array's start.
        let offset = fields.as_ptr() as usize - start;
        let pad = (8 - offset % 8) % 8;
        if pad >= fields.len() {
            break;
        }
        fields = &fields[pad..];
        if fields.len() < 2 {
            break;
        }
        let code = fields[0];
        let sig_len = fields[1] as usize;
        if fields.len() < 2 + sig_len + 1 {
            break;
        }
        let sig = &fields[2..2 + sig_len];
        fields = &fields[2 + sig_len + 1..];

        match sig {
            b"u" => {
                let offset = fields.as_ptr() as usize - start;
                let pad = (4 - offset % 4) % 4;
                if fields.len() < pad + 4 {
                    break;
                }
                let v = read_u32_le(&fields[pad..pad + 4]);
                if code == FIELD_REPLY_SERIAL {
                    reply_serial = v;
                }
                fields = &fields[pad + 4..];
            }
            b"s" | b"o" => {
                let offset = fields.as_ptr() as usize - start;
                let pad = (4 - offset % 4) % 4;
                if fields.len() < pad + 4 {
                    break;
                }
                let len = read_u32_le(&fields[pad..pad + 4]) as usize;
                if fields.len() < pad + 4 + len + 1 {
                    break;
                }
                let value = String::from_utf8_lossy(&fields[pad + 4..pad + 4 + len]).into_owned();
                if code == FIELD_ERROR_NAME {
                    error_name = Some(value);
                } else if code == FIELD_MEMBER {
                    member = Some(value);
                }
                fields = &fields[pad + 4 + len + 1..];
            }
            b"g" => {
                let len = fields[0] as usize;
                if fields.len() < 1 + len + 1 {
                    break;
                }
                fields = &fields[1 + len + 1..];
            }
            _ => break,
        }
    }
    (reply_serial, error_name, member)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Layouts below are taken from the D-Bus specification's type system.

    #[test]
    fn string_is_length_bytes_nul() {
        let mut e = Encoder::new();
        e.string("foo");
        assert_eq!(e.into_bytes(), vec![3, 0, 0, 0, b'f', b'o', b'o', 0]);
    }

    #[test]
    fn signature_uses_a_single_length_byte() {
        let mut e = Encoder::new();
        e.signature("s");
        assert_eq!(e.into_bytes(), vec![1, b's', 0]);
    }

    #[test]
    fn u32_is_padded_to_four() {
        let mut e = Encoder::new();
        e.byte(1);
        e.u32(2);
        assert_eq!(e.into_bytes(), vec![1, 0, 0, 0, 2, 0, 0, 0]);
    }

    #[test]
    fn an_empty_array_is_just_a_zero_length() {
        let mut e = Encoder::new();
        e.array(4, |_| {});
        assert_eq!(e.into_bytes(), vec![0, 0, 0, 0]);
    }

    #[test]
    fn an_empty_dict_array_still_pads_to_eight() {
        // The bug this pins: a{sv} elements align to 8, and the padding after
        // the length is required even with no elements. Omitting it makes the
        // bus reject the message with no useful diagnostic.
        let mut e = Encoder::new();
        e.array(8, |_| {});
        assert_eq!(e.into_bytes(), vec![0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn array_length_counts_only_the_elements() {
        let mut e = Encoder::new();
        e.array(4, |a| {
            a.string("ab");
        });
        // length(4) = 7 bytes of element data: 4 length + "ab" + NUL
        assert_eq!(e.into_bytes(), vec![7, 0, 0, 0, 2, 0, 0, 0, b'a', b'b', 0]);
    }

    #[test]
    fn structs_align_to_eight() {
        let mut e = Encoder::new();
        e.byte(9);
        e.structure(|s| s.byte(1));
        let out = e.into_bytes();
        assert_eq!(out.len(), 9);
        assert_eq!(out[8], 1);
    }

    #[test]
    fn notify_body_matches_the_expected_signature_layout() {
        // Not a byte-for-byte fixture, but the shape must hold: eight fields,
        // ending with a 4-byte -1, and 8-aligned before it because a{sv} pads.
        let mut b = Encoder::new();
        b.string("Dropkick");
        b.u32(0);
        b.string("");
        b.string("summary");
        b.string("body");
        b.array(4, |_| {});
        b.array(8, |_| {});
        b.i32(-1);
        let out = b.into_bytes();
        assert_eq!(&out[out.len() - 4..], &(-1i32).to_le_bytes());
        assert!(out.len() > 40);
    }

    #[test]
    fn alarm_hints_encode_as_a_populated_dict() {
        let mut e = Encoder::new();
        e.array(8, |h| {
            h.dict_entry_byte("urgency", 2);
            h.dict_entry_string("sound-name", "alarm-clock-elapsed");
        });
        let out = e.into_bytes();
        let declared = u32::from_le_bytes([out[0], out[1], out[2], out[3]]) as usize;
        assert!(declared > 0, "an alarm must not send an empty hint dict");
        // Length counts elements only, and the elements begin 8-aligned.
        assert_eq!(declared, out.len() - 8);
    }

    #[test]
    fn header_field_parsing_finds_a_reply_serial() {
        // Build one the way `call` does, then read it back.
        let mut fields = Encoder::new();
        fields.structure(|f| {
            f.byte(5); // REPLY_SERIAL
            f.signature("u");
            f.u32(42);
        });
        let (serial, err, _) = parse_header_fields(&fields.into_bytes());
        assert_eq!(serial, 42);
        assert!(err.is_none());
    }

    #[test]
    fn header_field_parsing_finds_an_error_name() {
        let mut fields = Encoder::new();
        fields.structure(|f| {
            f.byte(4); // ERROR_NAME
            f.signature("s");
            f.string("org.freedesktop.DBus.Error.ServiceUnknown");
        });
        let (_, err, _) = parse_header_fields(&fields.into_bytes());
        assert_eq!(err.as_deref(), Some("org.freedesktop.DBus.Error.ServiceUnknown"));
    }

    #[test]
    fn header_field_parsing_survives_truncation() {
        let mut fields = Encoder::new();
        fields.structure(|f| {
            f.byte(5);
            f.signature("u");
            f.u32(42);
        });
        let bytes = fields.into_bytes();
        for cut in 0..bytes.len() {
            let _ = parse_header_fields(&bytes[..cut]);
        }
    }
}

#[cfg(test)]
mod pick_tests {
    use super::*;

    /// Build a plausible Response body: `(u, a{sv})` with one `color` entry
    /// holding `(ddd)`.
    fn body(response: u32, rgb: (f64, f64, f64)) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&response.to_le_bytes());
        // The array and dict framing is not what the decoder reads -- it scans
        // for the key and then for aligned doubles -- so it only has to be
        // shaped realistically, with the key present and the values 8-aligned.
        v.extend_from_slice(b"\0\0\0\0"); // array length placeholder
        v.extend_from_slice(b"color\0");
        while v.len() % 8 != 0 {
            v.push(0);
        }
        for c in [rgb.0, rgb.1, rgb.2] {
            v.extend_from_slice(&c.to_le_bytes());
        }
        v
    }

    #[test]
    fn a_successful_response_yields_the_colour() {
        let PickResult::Picked(r, g, b) = parse_pick_response(&body(0, (0.5, 0.25, 1.0))) else {
            panic!("should decode");
        };
        assert!((r - 0.5).abs() < 1e-12);
        assert!((g - 0.25).abs() < 1e-12);
        assert!((b - 1.0).abs() < 1e-12);
    }

    #[test]
    fn cancelling_and_failing_to_decode_are_told_apart() {
        // Response code 1 is "user cancelled". Not an error, and definitely not
        // a colour -- reading the payload anyway would invent one.
        assert_eq!(parse_pick_response(&body(1, (0.5, 0.5, 0.5))), PickResult::Cancelled(1));
        assert_eq!(parse_pick_response(&body(2, (0.5, 0.5, 0.5))), PickResult::Cancelled(2));
    }

    #[test]
    fn a_truncated_or_empty_body_does_not_panic() {
        let full = body(0, (0.1, 0.2, 0.3));
        for n in 0..full.len() {
            let _ = parse_pick_response(&full[..n]);
        }
        assert_eq!(parse_pick_response(&[]), PickResult::Undecodable);
    }

    #[test]
    fn values_outside_zero_to_one_are_rejected_as_a_bad_scan() {
        // The decoder scans for aligned doubles, so its guard against landing
        // on padding is that colour components are 0..=1.
        let bad = body(0, (5.0, -1.0, 1e30));
        assert_eq!(parse_pick_response(&bad), PickResult::Undecodable);
    }

    #[test]
    fn a_body_without_the_key_finds_nothing() {
        let mut v = Vec::new();
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(b"something else entirely\0");
        assert_eq!(parse_pick_response(&v), PickResult::Undecodable);
    }
}
