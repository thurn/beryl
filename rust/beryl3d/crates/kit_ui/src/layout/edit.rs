//! Editing the focused text field in place (docs/beryl-3d.md, "Text" and "Hosts": on the
//! desktop a field is edited in place). The field's text, caret and selection are held by
//! parley's `PlainEditor`, which moves the caret, selects and edits over the field's laid-out
//! text. While a field is edited the layer draws the editor's layout in place of the field's
//! own text, with the selection, the caret and the underline of an input method's composing
//! text as panels (`write`). Like the rest of the layer's work, this happens in
//! `Client::handle`, never in a frame.

use glam::Vec2;
use parley::{Affinity, BoundingBox, Cursor, PlainEditor, Selection};

use super::{Layer, Node};
use crate::input::{Key, KeyCode, KeyPhase, Rect};
use kit_frame::frame::ElementId;
use kit_text::text::{self, Run};

/// The width of the caret, in logical points.
const CARET_WIDTH: f32 = 1.5;
/// The thickness of the line under an input method's composing text, in logical points.
const UNDERLINE: f32 = 1.5;

/// The field being edited.
pub(super) struct Editing {
    pub(super) id: ElementId,
    editor: PlainEditor<u16>,
    /// The field's text as its element last gave it, so that a new text from the screen
    /// replaces the editor's and an echo of the editor's own change does not.
    given: String,
    /// What is drawn over the field's text.
    pub(super) marks: Marks,
    /// Whether the primary button went down in the field and has not come up, so that moving
    /// the pointer extends the selection.
    dragging: bool,
}

impl Editing {
    /// The text the field shows while it is edited, its placeholder when it is empty: what its
    /// block was shaped from (`edit_block`).
    pub(super) fn shown<'a>(&'a self, node: &'a Node) -> &'a str {
        match self.editor.raw_text() {
            "" => node.element.text.as_deref().unwrap_or(""),
            raw => raw,
        }
    }
}

/// The selection, caret and composing underline of the edited field, in logical points from
/// the top left corner of its laid-out text.
#[derive(Debug, Default)]
pub(super) struct Marks {
    pub(super) selection: Vec<Rect>,
    pub(super) caret: Option<Rect>,
    pub(super) underline: Option<Rect>,
}

/// One edit of the focused field.
#[derive(Clone, Copy, Debug)]
pub enum Edit<'a> {
    /// Text typed, or committed by an input method, inserted at the caret.
    Insert(&'a str),
    /// The field's whole new text, from a phone's native field.
    Replace(&'a str),
    /// An input method's composing text, shown at the caret; an empty one ends composing.
    Preedit(&'a str),
    /// The clipboard's text, inserted at the caret.
    Paste(&'a str),
    /// A key, on the Mac if the flag is set: `Cmd` is the Command key there and Control
    /// elsewhere.
    Key(Key, bool),
    /// The primary button went down at a point of the window: the second press of a double
    /// click selects a word, and one with Shift held extends the selection.
    Down {
        point: Vec2,
        word: bool,
        extend: bool,
    },
    /// The pointer moved to a point of the window.
    Move(Vec2),
    /// The primary button came up.
    Up,
}

/// What an edit asks of the client.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Edited {
    /// The field's new text, if the edit changed it.
    pub text: Option<String>,
    /// Text to put on the clipboard: the selection, copied or cut.
    pub copy: Option<String>,
    /// Whether to read the clipboard, to paste what it holds.
    pub paste: bool,
    /// Whether the field took the edit, so that a key means nothing else.
    pub took: bool,
}

impl Layer {
    /// The edited field, its box on the window and its text, if the focus is on a text field.
    #[must_use]
    pub fn edited(&self) -> Option<(ElementId, Rect, String)> {
        let editing = self.editing.as_ref()?;
        let node = self.find(editing.id)?;
        let origin = self.origin_of(editing.id)? + node.offset();
        let rect = Rect {
            x: origin.x + node.rect.x,
            y: origin.y + node.rect.y,
            ..node.rect
        };
        Some((editing.id, rect, committed(&editing.editor)))
    }

    /// The text field being edited, if the focus is on one.
    #[must_use]
    pub fn edited_id(&self) -> Option<ElementId> {
        self.editing.as_ref().map(|editing| editing.id)
    }

    /// Whether a text field takes a press at this point.
    #[must_use]
    pub fn field_at(&self, point: Vec2, open: bool) -> bool {
        self.hit(point, open, |node, _| {
            node.element.field.as_ref().map(|_| ())
        })
        .is_some()
    }

    /// The name of the screen's element `id`, if it is on the screen.
    #[must_use]
    pub fn name_of(&self, id: ElementId) -> Option<&str> {
        self.find(id).map(|node| node.element.name.as_str())
    }

    /// The edited field's caret and selection, as byte offsets of its text.
    #[must_use]
    pub fn edited_caret(&self) -> Option<(u32, Option<std::ops::Range<u32>>)> {
        let editor = &self.editing.as_ref()?.editor;
        let range = editor.raw_selection().text_range();
        let compose = editor.raw_compose().clone();
        // Offsets past the composing text are offsets of the text without it.
        let plain = |offset: usize| match &compose {
            Some(compose) if offset >= compose.end => offset - compose.len(),
            Some(compose) if offset > compose.start => compose.start,
            _ => offset,
        };
        let caret = plain(editor.raw_selection().focus().index());
        let (start, end) = (plain(range.start), plain(range.end));
        let wide = |offset: usize| u32::try_from(offset).expect("a field holds under 4 GB");
        let selection = (start < end).then(|| wide(start)..wide(end));
        Some((wide(caret), selection))
    }

    /// Starts editing the focused element if it is a text field, or stops editing one that has
    /// lost the focus or left the screen. Called whenever the focus or the screen changes.
    pub(super) fn follow_focus(&mut self) {
        let wanted = self.focused.and_then(|id| {
            let node = self.find(id)?;
            let style = node.element.style.text;
            node.element
                .field
                .as_ref()
                .map(|field| (id, field.clone(), style))
        });
        match (&mut self.editing, wanted) {
            (Some(editing), Some((id, field, _))) if editing.id == id => {
                if editing.given != field.text {
                    editing.given.clone_from(&field.text);
                    if committed(&editing.editor) != field.text {
                        editing.editor.set_text(&field.text);
                    }
                }
            }
            (_, wanted) => {
                if let Some(old) = self.editing.take() {
                    self.restore_block(old.id);
                }
                if let Some((id, field, style)) = wanted {
                    let mut editor = self.text.editor(style);
                    editor.set_text(&field.text);
                    let (fonts, layouts) = self.text.contexts();
                    let mut driver = editor.driver(fonts, layouts);
                    match &field.selection {
                        Some(range) => {
                            driver.select_byte_range(range.start as usize, range.end as usize);
                        }
                        None => driver.move_to_byte(field.caret as usize),
                    }
                    self.editing = Some(Editing {
                        id,
                        editor,
                        given: field.text.clone(),
                        marks: Marks::default(),
                        dragging: false,
                    });
                }
            }
        }
        self.refresh_edit();
    }

    /// Edits the focused field, if a text field has the focus.
    pub fn edit(&mut self, edit: Edit) -> Edited {
        let origin = (self.editing.as_ref()).and_then(|editing| self.text_origin(editing.id));
        let Some(editing) = &mut self.editing else {
            return Edited::default();
        };
        let before = committed(&editing.editor);
        let composing = editing.editor.is_composing();
        let mut edited = Edited {
            took: true,
            ..Edited::default()
        };
        let (fonts, layouts) = self.text.contexts();
        let mut driver = editing.editor.driver(fonts, layouts);
        match edit {
            Edit::Insert(words) | Edit::Paste(words) => {
                driver.clear_compose();
                driver.insert_or_replace_selection(words);
            }
            Edit::Replace(words) => {
                driver.editor.set_text(words);
                driver.move_to_text_end();
            }
            Edit::Preedit("") => driver.clear_compose(),
            Edit::Preedit(words) => driver.set_compose(words, Some((words.len(), words.len()))),
            // While an input method composes, its keys are its own, and the pointer leaves the
            // composing text where it is.
            Edit::Key(..) | Edit::Down { .. } | Edit::Move(_) if composing => {}
            Edit::Key(key, mac) => edited = key_edit(&mut driver, key, mac),
            Edit::Down {
                point,
                word,
                extend,
            } => {
                let Some(origin) = origin else {
                    return Edited::default();
                };
                let at = point - origin;
                if word {
                    driver.select_word_at_point(at.x, at.y);
                } else if extend {
                    driver.shift_click_extension(at.x, at.y);
                } else {
                    driver.move_to_point(at.x, at.y);
                }
                editing.dragging = true;
            }
            Edit::Move(point) => match origin {
                Some(origin) if editing.dragging => {
                    let at = point - origin;
                    driver.extend_selection_to_point(at.x, at.y);
                }
                _ => edited.took = false,
            },
            Edit::Up => editing.dragging = false,
        }
        let after = committed(&editing.editor);
        if after != before {
            editing.given.clone_from(&after);
            edited.text = Some(after);
        }
        self.refresh_edit();
        edited
    }

    /// Lays the edited field's text out again with its editor, at its content box's width,
    /// and measures the marks drawn over it.
    pub(super) fn refresh_edit(&mut self) {
        let Some(editing) = &mut self.editing else {
            return;
        };
        let Some(node) = find_mut(&mut self.screen, editing.id) else {
            self.editing = None;
            return;
        };
        let width = Some(node.content.width + 0.01);
        editing.editor.set_width(width);
        let (fonts, layouts) = self.text.contexts();
        editing.editor.refresh_layout(fonts, layouts);
        let marks = &mut editing.marks;
        marks.underline = editing.editor.raw_compose().clone().map(|compose| {
            let layout = editing.editor.try_layout().expect("laid out above");
            let selection = Selection::new(
                Cursor::from_byte_index(layout, compose.start, Affinity::Downstream),
                Cursor::from_byte_index(layout, compose.end, Affinity::Upstream),
            );
            // One line under all of it, at the bottom of its last line.
            let (mut left, mut right, mut bottom) = (f32::MAX, f32::MIN, f32::MIN);
            for (bounds, _) in selection.geometry(layout) {
                let line = rect_of(bounds);
                left = left.min(line.x);
                right = right.max(line.x + line.width);
                bottom = bottom.max(line.y + line.height);
            }
            Rect {
                x: left,
                y: bottom - UNDERLINE,
                width: (right - left).max(0.0),
                height: UNDERLINE,
            }
        });
        let selection = editing.editor.raw_selection();
        marks.selection.clear();
        if !selection.is_collapsed() {
            (marks.selection).extend(
                (editing.editor.selection_geometry().into_iter())
                    .map(|(bounds, _)| rect_of(bounds)),
            );
        }
        marks.caret = (selection.is_collapsed())
            .then(|| editing.editor.cursor_geometry(CARET_WIDTH).map(rect_of))
            .flatten();
        let layout = editing.editor.try_layout().expect("laid out above");
        node.block = if editing.editor.raw_text().is_empty() {
            // An empty field shows its placeholder, which is its element's text.
            (node.element.text.as_ref()).map(|words| {
                let style = node.element.style.text;
                (self.text.block(words, style, &node.element.spans, width)).clone()
            })
        } else {
            let base = Run {
                style: node.element.style.text,
                color: None,
            };
            Some(text::plain_block(layout, base, width))
        };
        if let Some(block) = &node.block {
            let shadows = &node.element.style.drop_shadow;
            self.glyphs.want_with(block, self.window.scale, shadows);
        }
    }

    /// Gives the field `id`, no longer edited, its own text again.
    fn restore_block(&mut self, id: ElementId) {
        let Some(node) = find_mut(&mut self.screen, id) else {
            return;
        };
        let width = Some(node.content.width + 0.01);
        node.block = (node.element.text.as_ref()).map(|words| {
            let style = node.element.style.text;
            (self.text.block(words, style, &node.element.spans, width)).clone()
        });
    }

    /// Where the top left corner of the text field `id`'s laid-out text stands on the window,
    /// as `write` draws it: at the top left of its content box, moved by the node's transition
    /// and its ancestors' scroll.
    fn text_origin(&self, id: ElementId) -> Option<Vec2> {
        let node = self.find(id)?;
        let point = self.origin_of(id)? + node.offset();
        Some(point + Vec2::new(node.content.x, node.content.y))
    }

    /// Where the tree that the screen node `id`'s box is laid out in stands on the window: the
    /// point `write` draws its box from.
    fn origin_of(&self, id: ElementId) -> Option<Vec2> {
        fn search(node: &Node, id: ElementId, origin: Vec2) -> Option<Vec2> {
            if node.id == id {
                return Some(origin);
            }
            let inside = origin - node.scroll;
            node.children
                .iter()
                .find_map(|child| match child.element.anchor {
                    Some(_) => search(child, id, child.corner()?),
                    None => search(child, id, inside),
                })
        }
        let screen = self.screen.as_ref()?;
        let origin = match screen.element.anchor {
            Some(_) => screen.corner()?,
            None => Vec2::ZERO,
        };
        search(screen, id, origin)
    }
}

/// What `key` does to the field `driver` edits, on the Mac if `mac`.
fn key_edit(driver: &mut parley::PlainEditorDriver<'_, u16>, key: Key, mac: bool) -> Edited {
    let mut edited = Edited {
        took: true,
        ..Edited::default()
    };
    if key.phase == KeyPhase::Up {
        edited.took = false;
        return edited;
    }
    let held = key.modifiers;
    let command = if mac { held.command } else { held.control };
    // A word at a time: Option on the Mac, Control elsewhere.
    let word = if mac { held.alt } else { held.control };
    let shift = held.shift;
    match key.code {
        KeyCode::A if command => driver.select_all(),
        KeyCode::C if command => edited.copy = driver.editor.selected_text().map(str::to_owned),
        KeyCode::X if command => {
            edited.copy = driver.editor.selected_text().map(str::to_owned);
            if edited.copy.is_some() {
                driver.delete_selection();
            }
        }
        KeyCode::V if command => edited.paste = true,
        // Cmd+Left and Cmd+Right go to the line's ends on the Mac.
        KeyCode::Home | KeyCode::ArrowLeft if key.code == KeyCode::Home || (mac && command) => {
            if shift {
                driver.select_to_line_start();
            } else {
                driver.move_to_line_start();
            }
        }
        KeyCode::End | KeyCode::ArrowRight if key.code == KeyCode::End || (mac && command) => {
            if shift {
                driver.select_to_line_end();
            } else {
                driver.move_to_line_end();
            }
        }
        KeyCode::ArrowLeft => match (shift, word) {
            (true, true) => driver.select_word_left(),
            (true, false) => driver.select_left(),
            (false, true) => driver.move_word_left(),
            (false, false) => driver.move_left(),
        },
        KeyCode::ArrowRight => match (shift, word) {
            (true, true) => driver.select_word_right(),
            (true, false) => driver.select_right(),
            (false, true) => driver.move_word_right(),
            (false, false) => driver.move_right(),
        },
        KeyCode::ArrowUp if shift => driver.select_up(),
        KeyCode::ArrowUp => driver.move_up(),
        KeyCode::ArrowDown if shift => driver.select_down(),
        KeyCode::ArrowDown => driver.move_down(),
        KeyCode::Backspace if word => driver.backdelete_word(),
        KeyCode::Backspace => driver.backdelete(),
        KeyCode::Delete if word => driver.delete_word(),
        KeyCode::Delete => driver.delete(),
        _ => edited.took = false,
    }
    edited
}

/// The editor's text without an input method's composing text.
fn committed(editor: &PlainEditor<u16>) -> String {
    let raw = editor.raw_text();
    match editor.raw_compose() {
        Some(compose) => [&raw[..compose.start], &raw[compose.end..]].concat(),
        None => raw.to_owned(),
    }
}

/// A box parley measured, as a rectangle in logical points.
#[expect(
    clippy::cast_possible_truncation,
    reason = "parley measures in f64 what it laid out in f32"
)]
fn rect_of(bounds: BoundingBox) -> Rect {
    Rect {
        x: bounds.x0 as f32,
        y: bounds.y0 as f32,
        width: (bounds.x1 - bounds.x0) as f32,
        height: (bounds.y1 - bounds.y0) as f32,
    }
}

/// The node `id` of the screen's tree.
fn find_mut(screen: &mut Option<Node>, id: ElementId) -> Option<&mut Node> {
    fn search(node: &mut Node, id: ElementId) -> Option<&mut Node> {
        if node.id == id {
            return Some(node);
        }
        node.children.iter_mut().find_map(|child| search(child, id))
    }
    screen.as_mut().and_then(|node| search(node, id))
}
