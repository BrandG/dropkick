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

fn read_u32_le(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
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

        let mut bus = Bus { stream: UnixStream::connect(path)?, serial: 0 };
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
        self.call(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "Hello",
            "",
            Vec::new(),
        )?;
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
            let (msg_type, reply_serial, body, error) = self.read_message()?;
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

    /// Returns (type, reply_serial, body, error_name).
    fn read_message(&mut self) -> io::Result<(u8, u32, Vec<u8>, Option<String>)> {
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

        let (reply_serial, error_name) = parse_header_fields(&rest[..fields_len]);
        let body = rest[padded_fields..].to_vec();
        Ok((msg_type, reply_serial, body, error_name))
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
fn parse_header_fields(mut fields: &[u8]) -> (u32, Option<String>) {
    const FIELD_REPLY_SERIAL: u8 = 5;
    let mut reply_serial = 0u32;
    let mut error_name = None;
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
    (reply_serial, error_name)
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
        let (serial, err) = parse_header_fields(&fields.into_bytes());
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
        let (_, err) = parse_header_fields(&fields.into_bytes());
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
