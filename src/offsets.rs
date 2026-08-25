// UTF-8 byte offset -> UTF-16 code-unit offset translation (compatibility
// contract §5.5, the port design notes).
//
// web-tree-sitter (WASM, what the Node implementation loads) parses a JS string
// and reports spans in UTF-16 code units. Native tree-sitter parses a UTF-8 byte
// buffer and reports spans in UTF-8 bytes. Rows always agree between the two and
// CRLF is harmless either way -- only byte offsets and byte-relative columns
// diverge, and only across multi-byte characters:
//   - a 2-byte UTF-8 char is 1 UTF-16 unit               -> divergence -1
//   - a 3-byte UTF-8 char (incl. an unstripped BOM) is 1  -> divergence -2
//   - a 4-byte UTF-8 char (astral, UTF-16 surrogate pair) -> divergence -2

/// Maps UTF-8 byte offsets in one source file to UTF-16 code-unit offsets.
pub struct OffsetTable {
    /// (byte offset immediately after a multi-byte char, cumulative
    /// UTF-8-bytes-minus-UTF-16-units divergence up to and including that
    /// char). Sorted ascending by construction (`char_indices` is
    /// monotonic); empty for ASCII-only source, where byte offset always
    /// equals UTF-16 offset.
    breakpoints: Vec<(usize, i64)>,
}

impl OffsetTable {
    pub fn build(src: &str) -> Self {
        let mut breakpoints = Vec::new();
        let mut divergence: i64 = 0;
        for (byte_start, ch) in src.char_indices() {
            let utf8_len = ch.len_utf8() as i64;
            let utf16_len = ch.len_utf16() as i64;
            if utf8_len != utf16_len {
                divergence += utf8_len - utf16_len;
                breakpoints.push((byte_start + ch.len_utf8(), divergence));
            }
        }
        OffsetTable { breakpoints }
    }

    /// Cumulative divergence in effect at `byte_offset` (i.e. contributed by
    /// every multi-byte char that ends at or before it).
    fn divergence_at(&self, byte_offset: usize) -> i64 {
        match self
            .breakpoints
            .binary_search_by(|(pos, _)| pos.cmp(&byte_offset))
        {
            Ok(idx) => self.breakpoints[idx].1,
            Err(0) => 0,
            Err(idx) => self.breakpoints[idx - 1].1,
        }
    }

    /// Translate one UTF-8 byte offset (must land on a char boundary, which
    /// every tree-sitter node boundary does for valid UTF-8 source) to its
    /// UTF-16 code-unit offset from the start of the file.
    pub fn byte_to_utf16(&self, byte_offset: usize) -> usize {
        (byte_offset as i64 - self.divergence_at(byte_offset)) as usize
    }

    /// Translate a tree-sitter point -- `byte_offset` from the start of the
    /// file, `column_bytes` from the start of its row -- into
    /// (utf16 offset from file start, utf16 column from row start). Rows
    /// themselves are never translated; §5.5 rows always agree.
    pub fn translate_point(&self, byte_offset: usize, column_bytes: usize) -> (usize, usize) {
        let line_start = byte_offset - column_bytes;
        let offset_u16 = self.byte_to_utf16(byte_offset);
        let line_start_u16 = self.byte_to_utf16(line_start);
        (offset_u16, offset_u16 - line_start_u16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_has_no_divergence() {
        let src = "class Widget {}\n";
        let table = OffsetTable::build(src);
        for byte in 0..=src.len() {
            assert_eq!(table.byte_to_utf16(byte), byte);
        }
    }

    #[test]
    fn bom_diverges_by_two() {
        // U+FEFF (unstripped BOM): 3-byte UTF-8, 1 UTF-16 unit -> -2.
        let src = "\u{FEFF}class Widget {}";
        let table = OffsetTable::build(src);
        assert_eq!(table.byte_to_utf16(0), 0);
        assert_eq!(table.byte_to_utf16(3), 1); // right after the BOM
        assert_eq!(table.byte_to_utf16(3 + 5), 1 + 5); // "class" follows it
    }

    #[test]
    fn accented_char_diverges_by_one() {
        // 'e' with acute accent (U+00E9): 2-byte UTF-8, 1 UTF-16 unit -> -1.
        let src = "// caf\u{E9}\n";
        let table = OffsetTable::build(src);
        let accent_byte_start = 6; // "// caf" = 6 ASCII bytes before it
        assert_eq!(table.byte_to_utf16(accent_byte_start), 6);
        assert_eq!(table.byte_to_utf16(accent_byte_start + 2), 7);
    }

    #[test]
    fn cjk_char_diverges_by_two() {
        // U+4E2D ('中'): 3-byte UTF-8, 1 UTF-16 unit -> -2.
        let src = "// \u{4E2D}\n";
        let table = OffsetTable::build(src);
        assert_eq!(table.byte_to_utf16(3), 3); // before the CJK char
        assert_eq!(table.byte_to_utf16(3 + 3), 4);
    }

    #[test]
    fn astral_emoji_diverges_by_two_via_surrogate_pair() {
        // U+1F600 (emoji): 4-byte UTF-8, 2 UTF-16 units (surrogate pair) -> -2.
        let src = "// \u{1F600}\n";
        let table = OffsetTable::build(src);
        assert_eq!(table.byte_to_utf16(3), 3);
        assert_eq!(table.byte_to_utf16(3 + 4), 3 + 2);
    }

    #[test]
    fn multiple_multibyte_chars_accumulate() {
        let src = "\u{00E9}\u{00E9}"; // two 2-byte chars back to back
        let table = OffsetTable::build(src);
        assert_eq!(table.byte_to_utf16(0), 0);
        assert_eq!(table.byte_to_utf16(2), 1);
        assert_eq!(table.byte_to_utf16(4), 2);
    }

    #[test]
    fn column_ignores_divergence_from_earlier_lines() {
        // Line 1 has two 2-byte accented chars (-1 each, -2 total by its
        // end). Line 2 is pure ASCII: its column must not inherit line 1's
        // divergence, only the file-global byte offset should.
        let src = "// \u{00E9}\u{00E9}\nclass Widget {}\n";
        let table = OffsetTable::build(src);
        let widget_byte = src.find("Widget").unwrap();
        let line_start_byte = src.find("class").unwrap();
        let column_bytes = widget_byte - line_start_byte;
        assert_eq!(column_bytes, 6);

        let (offset_u16, column_u16) = table.translate_point(widget_byte, column_bytes);
        assert_eq!(column_u16, 6, "line 2 is ASCII; column must equal raw byte column");
        assert_eq!(offset_u16, widget_byte - 2, "global offset reflects line 1's -2 divergence");
    }
}
