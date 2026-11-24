//! Every character a key hint can show has a glyph in the interface font.

use crate::element::describe;
use kit_text::text::fonts::Font;
use swash::FontRef;

/// Characters known to draw nothing. Empty: key hints spell their modifiers out.
const KNOWN_MISSING: &[char] = &[];

#[test]
fn key_hints_draw_in_the_interface_font() {
    let font = FontRef::from_index(Font::OpenSans.file().data, 0).unwrap();
    let modifiers = [
        "",
        "Cmd+",
        "Ctrl+",
        "Option+",
        "Shift+",
        "Cmd+Shift+",
        "Ctrl+Option+Shift+Cmd+",
    ];
    let keys = [
        "A",
        "Z",
        "0",
        "9",
        "Backspace",
        "Delete",
        "Enter",
        "Escape",
        "Tab",
        "Space",
        "ArrowUp",
    ];
    for mac in [true, false] {
        for modifier in modifiers {
            for key in keys {
                let hint = describe(&format!("{modifier}{key}"), mac);
                for character in hint.chars() {
                    let drawn = font.charmap().map(character) != 0;
                    assert!(
                        drawn || KNOWN_MISSING.contains(&character),
                        "{hint:?} has {character:?} ({:#x}), which no bundled font draws",
                        u32::from(character)
                    );
                }
            }
        }
    }
}
