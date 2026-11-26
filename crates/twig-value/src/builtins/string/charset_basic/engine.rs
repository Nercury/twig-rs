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
//!   the replacement then does not fit.

use super::codec::{Charset, DecodeState, Decoded, Encoded};
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
    /// The characters of the current round with the input offset after each.
    chars: Vec<(u32, usize)>,
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
        chars: &mut Vec<(u32, usize)>,
    ) -> (usize, Status) {
        let mut round_start = 0;
        loop {
            // step 1: decode up to CHUNK characters
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
            while stop.is_none() && chars.len() < CHUNK && p < input.len() {
                match self.from.decode(&input[p..], &self.decode_state) {
                    Decoded::Char(c, len) => {
                        p += len;
                        chars.push((c, p));
                    }
                    Decoded::Invalid { len, silent } if self.ignore => {
                        p += len;
                        skipped.input |= !silent;
                    }
                    Decoded::Invalid { .. } => stop = Some(Status::Illegal),
                    Decoded::Incomplete => stop = Some(Status::Incomplete),
                }
            }

            // step 2: encode them
            if !chars.is_empty() {
                let first_run = !self.encoder_started;
                let bom = self.to.bom();
                if first_run && !bom.is_empty() {
                    if *space < bom.len() {
                        return (round_start, Status::TooBig);
                    }
                    out.extend_from_slice(bom);
                    *space -= bom.len();
                }
                self.encoder_started = true;
                for i in 0..chars.len() {
                    let before = if i == 0 { round_start } else { chars[i - 1].1 };
                    if let Err(status) =
                        self.encode(chars[i].0, first_run, out, space, &mut skipped)
                    {
                        return (before, status);
                    }
                }
            }

            if let Some(status) = stop {
                return (
                    p,
                    if skipped.output {
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

    /// Encodes one character of a round; `first_run`: the encoder's first run (see the module documentation).
    fn encode(
        &self,
        c: u32,
        first_run: bool,
        out: &mut Vec<u8>,
        space: &mut usize,
        skipped: &mut Skipped,
    ) -> Result<(), Status> {
        if *space < self.to.unit() {
            return Err(Status::TooBig);
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
                for r in translit::replacement(self.to, c).chars() {
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
                *skipped = Skipped::default();
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
