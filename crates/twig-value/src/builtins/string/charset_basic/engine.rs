//! One call of glibc's `iconv(3)` for two charsets of this module, as the oracle shows it (observed through PHP and
//! through a small C program calling `iconv()` in the oracle image; see tests/vectors/string/charset_basic).
//!
//! glibc converts in two steps, from the source charset to its internal UCS-4 and from there to the target, through
//! a buffer of [`CHUNK`] characters: each round decodes up to that many characters, then encodes them. What a call
//! reports depends on these rounds:
//!
//! - an error stops the call where it happened, the first one in input order winning: a character the encoder rejects
//!   (`EILSEQ`), an invalid input sequence (`EILSEQ`) or a character cut short by the end of the input (`EINVAL`);
//! - the call stops with `E2BIG` before a character when less than the target's unit is left in the output buffer
//!   (checked before every character, also one that is then skipped), or less than the character needs; a
//!   //TRANSLIT replacement is written whole or not at all;
//! - with //IGNORE, invalid input and characters the target cannot represent are skipped. A round in which the
//!   encoder skipped something ends the call with `EILSEQ` right after that round, before the rest of the input
//!   (at the end of the input too); input the decoder skipped only makes the call end with `EILSEQ` when it was
//!   skipped in the last round, and an incomplete character at the end still gives `EINVAL` unless the encoder
//!   skipped something in that round. A //TRANSLIT replacement (even an empty one) makes the round forget what was
//!   skipped before it (all the decoder skipped, since the decoder runs first). Some modules skip silently (see
//!   [`Decoded::Invalid`], [`Encoded::Illegal`]);
//! - the tag characters U+E0000..U+E007F vanish silently when the target cannot represent them, with or without
//!   //TRANSLIT and //IGNORE;
//! - UTF-16 and UTF-32 write their byte order mark when the encoder runs for the first time (a run that finds less
//!   room than the mark does not count), and during that first run again before every //TRANSLIT replacement (glibc
//!   converts the replacement as if it were the start of a stream); that second mark stays in the output even when
//!   the replacement then does not fit;
//! - the decoders of CP1255 and CP1258 hold characters back (see `codepage.rs`). A round needs room for a character
//!   before each byte it reads, and a held character only takes room once written: a byte that makes the decoder
//!   write the held character and then its own ends the round in between when only the first fits (the next round
//!   reads the byte again, with nothing held). When the encoder stops a call, the call ends after the last character
//!   written, with what the decoder held at that point. What the decoder holds at the end of the input stays in the
//!   descriptor until the next call or the flush (`iconv(cd, NULL, NULL, ...)`), which encodes it as a round of its
//!   own: `E2BIG` keeps it for the next try, and //IGNORE skipping it makes the flush fail with `EILSEQ`.

use super::codec::{Charset, DecodeState, Decoded, Encoded};
use super::codepage::Codepage;
use super::translit;

/// Characters per round: the size of glibc's buffer between the two steps.
pub(crate) const CHUNK: usize = 8160;

/// How an `iconv()` call ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    /// The whole input was converted (return value other than `(size_t)-1`).
    Done,
    /// `E2BIG`: the output buffer is full.
    TooBig,
    /// `EILSEQ`: an invalid input sequence or a character the target rejects, or //IGNORE skipped something.
    Illegal,
    /// `EINVAL`: the input ends inside a character.
    Incomplete,
}

/// What //IGNORE skipped in the current round (silent skips aside).
#[derive(Clone, Copy, Debug, Default)]
struct Skipped {
    /// The decoder skipped invalid input.
    input: bool,
    /// The encoder skipped a character the target rejects or cannot represent.
    output: bool,
    /// The encoder wrote a //TRANSLIT replacement.
    translit: bool,
}

/// A character of the current round.
#[derive(Clone, Copy, Debug)]
struct RoundChar {
    /// The code point.
    c: u32,
    /// Where the call ends when the encoder stops right after this character: the input offset after the byte that
    /// gave it (before that byte when the byte gives a second character, which does not fit then) ...
    resume: usize,
    /// ... and what a combining decoder holds back at that point.
    held: Option<u32>,
}

/// A conversion descriptor (`iconv_t`): the two charsets, the target's //TRANSLIT and //IGNORE flags and the state
/// that lives across calls.
#[derive(Debug)]
pub(crate) struct Converter {
    from: Charset,
    to: Charset,
    translit: bool,
    ignore: bool,
    decode_state: DecodeState,
    /// The encoder has run (once a round reached it with room for the byte order mark).
    encoder_started: bool,
    /// The character an encoder that composes holds back until the next one shows whether the two make one code
    /// (BIG5-HKSCS).
    encode_held: Option<u32>,
    /// The characters of the current round.
    chars: Vec<RoundChar>,
}

impl Converter {
    /// `iconv_open()` for the target `to` with its flags and the source `from`.
    pub(crate) fn new(from: Charset, to: Charset, translit: bool, ignore: bool) -> Self {
        Converter {
            from,
            to,
            translit,
            ignore,
            decode_state: DecodeState::default(),
            encoder_started: false,
            encode_held: None,
            chars: Vec::new(),
        }
    }

    /// One `iconv()` call on `input` (not empty), appending to `out` with `space` bytes left in the caller's buffer.
    /// Returns the number of input bytes consumed and how the call ended.
    pub(crate) fn call(
        &mut self,
        input: &[u8],
        out: &mut Vec<u8>,
        mut space: usize,
    ) -> (usize, Status) {
        let mut chars = std::mem::take(&mut self.chars);
        let result = self.call_rounds(input, out, &mut space, &mut chars);
        self.chars = chars;
        result
    }

    fn call_rounds(
        &mut self,
        input: &[u8],
        out: &mut Vec<u8>,
        space: &mut usize,
        chars: &mut Vec<RoundChar>,
    ) -> (usize, Status) {
        let mut round_start = 0;
        loop {
            // step 1: decode up to a round of characters
            chars.clear();
            let mut p = round_start;
            let mut skipped = Skipped::default();
            let mut stop = None;
            match self.from.read_bom(&input[p..], &mut self.decode_state) {
                Some(len) => {
                    p += len;
                    round_start = p;
                }
                None => stop = Some(Status::Incomplete),
            }
            let held_at_start = self.decode_state.held;
            if self.from.composing().is_none()
                && let Some(c) = self.decode_state.held.take()
            {
                // the second character of a pair (BIG5-HKSCS) the last call or round could not take comes first
                chars.push(RoundChar {
                    c,
                    resume: round_start,
                    held: None,
                });
            }
            let mut past_end = false;
            let mut moved_past = false;
            let chunk = self.from.chunk();
            while stop.is_none() && chars.len() < chunk && p < input.len() {
                match self
                    .from
                    .decode(&input[p..], &self.decode_state, self.ignore)
                {
                    Decoded::Pair(a, b, len) => {
                        // the decoder takes the sequence whole and keeps the second character for later when the
                        // round has room for the first only, or when the encoder stops after the first
                        p += len;
                        chars.push(RoundChar {
                            c: a,
                            resume: p,
                            held: Some(b),
                        });
                        if chars.len() == chunk {
                            self.decode_state.held = Some(b);
                        } else {
                            chars.push(RoundChar {
                                c: b,
                                resume: p,
                                held: None,
                            });
                        }
                    }
                    Decoded::Char(c, len) => match self.from.composing() {
                        Some(cp) => p = self.compose(cp, c, p, p + len, chars),
                        None => {
                            p += len;
                            chars.push(RoundChar {
                                c,
                                resume: p,
                                held: None,
                            });
                        }
                    },
                    Decoded::Invalid { len, silent } if self.ignore => {
                        p += len;
                        skipped.input |= !silent;
                    }
                    Decoded::Invalid { .. } => stop = Some(Status::Illegal),
                    Decoded::Incomplete => stop = Some(Status::Incomplete),
                    Decoded::PastEnd => {
                        stop = Some(Status::Incomplete);
                        past_end = true;
                    }
                    Decoded::IllegalPast(len) => {
                        p += len;
                        stop = Some(Status::Illegal);
                        moved_past = true;
                    }
                }
            }

            // step 2: encode them
            if !chars.is_empty() {
                let first_run = !self.encoder_started;
                let bom = self.to.bom();
                if first_run && !bom.is_empty() {
                    if *space < bom.len() {
                        self.decode_state.held = held_at_start;
                        return (round_start, Status::TooBig);
                    }
                    out.extend_from_slice(bom);
                    *space -= bom.len();
                }
                self.encoder_started = true;
                for i in 0..chars.len() {
                    if let Err(status) =
                        self.encode(chars[i].c, first_run, out, space, &mut skipped)
                    {
                        // the call stops after the last character written
                        let (resume, held) = match i {
                            0 => (round_start, held_at_start),
                            _ => (chars[i - 1].resume, chars[i - 1].held),
                        };
                        self.decode_state.held = held;
                        return (resume, status);
                    }
                }
            }

            if let Some(status) = stop {
                if moved_past && skipped.translit {
                    // a replacement makes glibc forget the error of a decoder that has moved on: the next round
                    // goes on after the sequence
                    round_start = p;
                    continue;
                }
                return (
                    p,
                    if skipped.output && !past_end {
                        Status::Illegal
                    } else {
                        status
                    },
                );
            }
            if p == input.len() {
                let any = skipped.input || skipped.output;
                return (p, if any { Status::Illegal } else { Status::Done });
            }
            if skipped.output {
                return (p, Status::Illegal);
            }
            round_start = p;
        }
    }

    /// Step 1 of a combining decoder for the character `c` of the byte at `start..end` (see `codepage.rs`). Returns
    /// where decoding goes on: before the byte when the round only had room for the character held before it.
    fn compose(
        &mut self,
        cp: Codepage,
        c: u32,
        start: usize,
        end: usize,
        chars: &mut Vec<RoundChar>,
    ) -> usize {
        let at = |c, resume, held| RoundChar { c, resume, held };
        let Some(held) = self.decode_state.held.take() else {
            if cp.holds(c) {
                self.decode_state.held = Some(c);
            } else {
                chars.push(at(c, end, None));
            }
            return end;
        };
        if let Some((combined, hold)) = cp.compose(held, c) {
            if hold {
                self.decode_state.held = Some(combined);
            } else {
                chars.push(at(combined, end, None));
            }
            return end;
        }
        if cp.holds(c) {
            self.decode_state.held = Some(c);
            chars.push(at(held, end, Some(c)));
            return end;
        }
        chars.push(at(held, start, None));
        if chars.len() == CHUNK {
            // no room left for `c`: the next round reads the byte again
            return start;
        }
        chars.push(at(c, end, None));
        end
    }

    /// `iconv(cd, NULL, NULL, ...)`, which PHP calls after converting: writes the character a combining decoder still
    /// holds, as a round of its own, then the one a composing encoder holds (`TooBig` keeps them for the next try).
    pub(crate) fn flush(&mut self, out: &mut Vec<u8>, mut space: usize) -> Status {
        let Some(held) = self.decode_state.held else {
            return self.flush_encoder(out, space);
        };
        let first_run = !self.encoder_started;
        let bom = self.to.bom();
        if first_run && !bom.is_empty() {
            if space < bom.len() {
                return Status::TooBig;
            }
            out.extend_from_slice(bom);
            space -= bom.len();
        }
        self.encoder_started = true;
        let mut skipped = Skipped::default();
        if let Err(status) = self.encode(held, first_run, out, &mut space, &mut skipped) {
            return status;
        }
        self.decode_state.held = None;
        if skipped.output {
            return Status::Illegal;
        }
        self.flush_encoder(out, space)
    }

    /// The flush of a composing encoder: writes the character it holds.
    fn flush_encoder(&mut self, out: &mut Vec<u8>, space: usize) -> Status {
        let Some(held) = self.encode_held else {
            return Status::Done;
        };
        let Encoded::Bytes(bytes, len) = self.to.encode(held) else {
            unreachable!("an encoder holds characters it can write")
        };
        if len > space {
            return Status::TooBig;
        }
        out.extend_from_slice(&bytes[..len]);
        self.encode_held = None;
        Status::Done
    }

    /// Encodes one character of a round; `first_run`: the encoder's first run (see the module documentation).
    fn encode(
        &mut self,
        c: u32,
        first_run: bool,
        out: &mut Vec<u8>,
        space: &mut usize,
        skipped: &mut Skipped,
    ) -> Result<(), Status> {
        if *space < self.to.unit() {
            return Err(Status::TooBig);
        }
        if let Some(held) = self.encode_held {
            // a composing encoder: the held character and this one make one code, or the held one is written first
            // and this one needs room for a unit again
            if let Some((bytes, len)) = self.to.encode_pair(held, c) {
                if len > *space {
                    return Err(Status::TooBig);
                }
                out.extend_from_slice(&bytes[..len]);
                *space -= len;
                self.encode_held = None;
                return Ok(());
            }
            let Encoded::Bytes(bytes, len) = self.to.encode(held) else {
                unreachable!("an encoder holds characters it can write")
            };
            if len > *space {
                return Err(Status::TooBig);
            }
            out.extend_from_slice(&bytes[..len]);
            *space -= len;
            self.encode_held = None;
            if *space < self.to.unit() {
                return Err(Status::TooBig);
            }
        }
        if self.to.encoder_holds(c) {
            self.encode_held = Some(c);
            return Ok(());
        }
        match self.to.encode(c) {
            Encoded::Bytes(bytes, len) => {
                if len > *space {
                    return Err(Status::TooBig);
                }
                out.extend_from_slice(&bytes[..len]);
                *space -= len;
            }
            // the tag characters vanish when the target cannot hold them, without //TRANSLIT or //IGNORE too
            Encoded::Unmappable if (0xE_0000..=0xE_007F).contains(&c) => {}
            Encoded::Unmappable if self.translit => {
                if first_run {
                    // the unit check above left room for the mark
                    let bom = self.to.bom();
                    out.extend_from_slice(bom);
                    *space -= bom.len();
                }
                let mut replacement = [0u8; 24];
                let mut len = 0;
                for &r in translit::replacement(self.to, c) {
                    let Encoded::Bytes(bytes, n) = self.to.encode(r) else {
                        unreachable!("transliterations are representable in their target")
                    };
                    replacement[len..len + n].copy_from_slice(&bytes[..n]);
                    len += n;
                }
                if len > *space {
                    return Err(Status::TooBig);
                }
                out.extend_from_slice(&replacement[..len]);
                *space -= len;
                // a replacement makes the round forget what //IGNORE skipped before it
                *skipped = Skipped {
                    translit: true,
                    ..Skipped::default()
                };
            }
            Encoded::Unmappable | Encoded::Illegal { silent: false } if self.ignore => {
                skipped.output = true
            }
            Encoded::Illegal { silent: true } if self.ignore => {}
            Encoded::Unmappable | Encoded::Illegal { .. } => return Err(Status::Illegal),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(
        from: Charset,
        to: Charset,
        translit: bool,
        ignore: bool,
        input: &[u8],
        space: usize,
    ) -> (usize, Status, Vec<u8>) {
        let mut conv = Converter::new(from, to, translit, ignore);
        let mut out = Vec::new();
        let (consumed, status) = conv.call(input, &mut out, space);
        (consumed, status, out)
    }

    #[test]
    fn rounds_and_ignore() {
        // the encoder skipped the euro sign in the first round: the call ends after that round
        let mut input = "\u{20ac}".as_bytes().to_vec();
        input.extend(std::iter::repeat_n(b'a', 9000));
        let (consumed, status, out) =
            run(Charset::Utf8, Charset::Latin1, false, true, &input, 100_000);
        assert_eq!((consumed, status, out.len()), (8162, Status::Illegal, 8159));
        // input skipped by the decoder in an earlier round is forgotten
        input[..3].copy_from_slice(b"\xff\xff\xff");
        let (consumed, status, _) = run(
            Charset::Utf8,
            Charset::Utf16Le,
            false,
            true,
            &input,
            100_000,
        );
        assert_eq!((consumed, status), (input.len(), Status::Done));
    }

    #[test]
    fn e2big_and_marks() {
        assert_eq!(
            run(Charset::Utf8, Charset::Utf16, false, false, b"aa", 1),
            (0, Status::TooBig, vec![])
        );
        assert_eq!(
            run(Charset::Utf8, Charset::Utf16, false, false, b"aa", 2),
            (0, Status::TooBig, vec![0xFF, 0xFE])
        );
        let input = [0, 0, 0, 0x61, 0, 0x11, 0, 0, 0, 0, 0, 0x62];
        assert_eq!(
            run(Charset::Ucs4, Charset::Utf16, true, false, &input, 6),
            (4, Status::TooBig, vec![0xFF, 0xFE, 0x61, 0, 0xFF, 0xFE])
        );
        assert_eq!(
            run(Charset::Ucs4, Charset::Utf16, true, false, &input, 100).2,
            [0xFF, 0xFE, 0x61, 0, 0xFF, 0xFE, 0x3F, 0, 0x62, 0]
        );
        assert_eq!(
            run(
                Charset::Utf8,
                Charset::Ascii,
                true,
                false,
                "\u{bd}".as_bytes(),
                4
            ),
            (0, Status::TooBig, vec![])
        );
    }

    #[test]
    fn mark_before_replacements_only_in_the_first_run() {
        let mut conv = Converter::new(Charset::Ucs4, Charset::Utf16, true, false);
        let mut out = Vec::new();
        let input = [0, 0, 0, 0x61, 0, 0x11, 0, 0, 0, 0, 0, 0x62];
        assert_eq!(conv.call(&input, &mut out, 4), (4, Status::TooBig));
        assert_eq!(conv.call(&input[4..], &mut out, 100), (8, Status::Done));
        assert_eq!(out, [0xFF, 0xFE, 0x61, 0, 0x3F, 0, 0x62, 0]);
    }

    #[test]
    fn held_characters() {
        let cp1258 = Charset::Codepage(Codepage::Cp1258);
        // CP1258 holds A, B and C back: E2BIG after A ends the call after B, which stays held
        let mut conv = Converter::new(cp1258, Charset::Ucs4, false, false);
        let mut out = Vec::new();
        assert_eq!(conv.call(b"ABC", &mut out, 4), (2, Status::TooBig));
        assert_eq!(conv.call(b"C", &mut out, 100), (1, Status::Done));
        assert_eq!(conv.flush(&mut out, 3), Status::TooBig);
        assert_eq!(conv.flush(&mut out, 4), Status::Done);
        assert_eq!(out, [0, 0, 0, 0x41, 0, 0, 0, 0x42, 0, 0, 0, 0x43]);
        // the mark after B does not combine with it: B is written, the mark does not fit and is read again
        let mut conv = Converter::new(cp1258, Charset::Ucs4, false, false);
        out.clear();
        assert_eq!(conv.call(b"AB\xcc", &mut out, 8), (2, Status::TooBig));
        assert_eq!(conv.call(b"\xcc", &mut out, 100), (1, Status::Done));
        assert_eq!(out, [0, 0, 0, 0x41, 0, 0, 0, 0x42, 0, 0, 3, 0]);
        // a held character is written by the flush, with the byte order mark if it is the first
        let mut conv = Converter::new(cp1258, Charset::Utf16, false, false);
        out.clear();
        assert_eq!(conv.call(b"A", &mut out, 100), (1, Status::Done));
        assert_eq!(conv.flush(&mut out, 100), Status::Done);
        assert_eq!(out, [0xFF, 0xFE, 0x41, 0]);
    }

    #[test]
    fn replacement_forgets_skips() {
        // a surrogate skipped, then a replacement: EINVAL from the cut-off end wins again
        let input = [0, 0, 0xD8, 0, 0, 0x11, 0, 0, 0x69];
        let (consumed, status, out) = run(Charset::Ucs4, Charset::Utf16Le, true, true, &input, 100);
        assert_eq!(
            (consumed, status, out),
            (8, Status::Incomplete, vec![0x3F, 0])
        );
        // the other order keeps the skip
        let input = [0, 0x11, 0, 0, 0, 0, 0xD8, 0, 0x69];
        let (_, status, _) = run(Charset::Ucs4, Charset::Utf16Le, true, true, &input, 100);
        assert_eq!(status, Status::Illegal);
    }
}
