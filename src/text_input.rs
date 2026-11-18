use crate::*;
use std::cell::RefCell;

#[derive(Default)]
struct Editor {
    cursor: usize,
    anchor: usize,
    start: usize,
    previous: String,
}
impl Editor {
    fn sync(&mut self, s: &str) {
        if self.previous != s {
            self.cursor = s.len();
            self.anchor = self.cursor;
            self.start = 0;
        }
    }
    fn range(&self) -> std::ops::Range<usize> {
        self.cursor.min(self.anchor)..self.cursor.max(self.anchor)
    }
    fn insert(&mut self, s: &mut String, value: &str) {
        let value: String = value.chars().filter(|c| !c.is_control()).collect();
        let range = self.range();
        self.cursor = range.start + value.len();
        self.anchor = self.cursor;
        s.replace_range(range, &value);
    }
    fn move_to(&mut self, to: usize, shift: bool) {
        self.cursor = to;
        if !shift {
            self.anchor = to;
        }
    }
}
thread_local! { static EDITORS: RefCell<BTreeMap<usize, Editor>> = RefCell::new(BTreeMap::new()); }
fn previous(s: &str, at: usize) -> usize {
    s[..at].char_indices().last().map(|(i, _)| i).unwrap_or(0)
}
fn following(s: &str, at: usize) -> usize {
    s[at..]
        .chars()
        .next()
        .map(|c| at + c.len_utf8())
        .unwrap_or(at)
}
fn display(s: &str, secret: bool) -> String {
    if secret {
        "•".repeat(s.chars().count())
    } else {
        s.into()
    }
}

pub fn input(id: usize, active: &mut usize, s: &mut String, r: Rect, placeholder: &str) {
    EDITORS.with(|editors| {
        let mut editors = editors.borrow_mut();
        let e = editors.entry(id).or_default();
        e.sync(s);
        let secret = id == 6;
        let width = |text: &str| metrics(&display(text, secret), 17).width;
        let available = (r.w - 22.).max(1.);
        let mouse = Vec2::from(mouse_position());
        let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
        let ctrl = is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl);
        if is_mouse_button_pressed(MouseButton::Left) {
            if r.contains(mouse) {
                *active = id;
                e.start = e.start.min(s.len());
                let at = s[e.start..]
                    .char_indices()
                    .map(|(i, c)| (e.start + i, c))
                    .find_map(|(i, c)| {
                        (mouse.x - r.x - 11. < width(&s[e.start..i]) + width(&c.to_string()) / 2.)
                            .then_some(i)
                    })
                    .unwrap_or(s.len());
                e.move_to(at, shift);
            }
        }
        if *active == id {
            if ctrl && is_key_pressed(KeyCode::A) {
                e.anchor = 0;
                e.cursor = s.len();
            }
            if ctrl && (is_key_pressed(KeyCode::C) || is_key_pressed(KeyCode::X)) && !secret {
                macroquad::miniquad::window::clipboard_set(&s[e.range()]);
                if is_key_pressed(KeyCode::X) {
                    e.insert(s, "");
                }
            }
            if ctrl && is_key_pressed(KeyCode::V) {
                if let Some(value) = macroquad::miniquad::window::clipboard_get() {
                    e.insert(s, &value);
                }
            }
            if is_key_pressed(KeyCode::Home) {
                e.move_to(0, shift);
            }
            if is_key_pressed(KeyCode::End) {
                e.move_to(s.len(), shift);
            }
            if is_key_pressed(KeyCode::Left) {
                let to = if !shift && !e.range().is_empty() {
                    e.range().start
                } else {
                    previous(s, e.cursor)
                };
                e.move_to(to, shift);
            }
            if is_key_pressed(KeyCode::Right) {
                let to = if !shift && !e.range().is_empty() {
                    e.range().end
                } else {
                    following(s, e.cursor)
                };
                e.move_to(to, shift);
            }
            if is_key_pressed(KeyCode::Backspace) {
                if e.range().is_empty() {
                    e.anchor = previous(s, e.cursor);
                }
                e.insert(s, "");
            }
            if is_key_pressed(KeyCode::Delete) {
                if e.range().is_empty() {
                    e.anchor = following(s, e.cursor);
                }
                e.insert(s, "");
            }
            while let Some(c) = get_char_pressed() {
                if !ctrl && !c.is_control() {
                    e.insert(s, &c.to_string());
                }
            }
            if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Escape) {
                *active = 0;
            }
        }
        e.start = e.start.min(e.cursor);
        while !s.is_char_boundary(e.start) {
            e.start -= 1;
        }
        while width(&s[e.start..e.cursor]) > available - 2. && e.start < e.cursor {
            e.start = following(s, e.start);
        }
        let mut end = s.len();
        while width(&s[e.start..end]) > available && end > e.start {
            end = previous(s, end);
        }
        panel(r.x, r.y, r.w, r.h, WHITE);
        draw_rectangle_lines(
            r.x,
            r.y,
            r.w,
            r.h,
            if *active == id { 2. } else { 1. },
            if *active == id { TEAL } else { LINE },
        );
        if *active == id {
            let selected = e.range();
            let left = selected.start.max(e.start).min(end);
            let right = selected.end.max(e.start).min(end);
            panel(
                r.x + 11. + width(&s[e.start..left]),
                r.y + 7.,
                width(&s[left..right]),
                r.h - 14.,
                Color::from_hex(0xc9e6e1),
            );
            let x = r.x + 11. + width(&s[e.start..e.cursor]);
            if (get_time() * 2.).fract() < 0.65 {
                draw_line(x, r.y + 9., x, r.y + r.h - 9., 1., INK);
            }
        }
        let shown = display(&s[e.start..end], secret);
        txt(
            if s.is_empty() { placeholder } else { &shown },
            r.x + 11.,
            r.y + 25.,
            17.,
            if s.is_empty() { MUTED } else { INK },
        );
        e.previous = s.clone();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacing_unicode_selection_and_deleting_at_edges() {
        let mut s = "ゲーム title".to_string();
        let mut e = Editor::default();
        e.sync(&s);
        e.move_to(0, false);
        e.insert(&mut s, "New ");
        assert_eq!(s, "New ゲーム title");
        e.anchor = 0;
        e.cursor = s.len();
        e.insert(&mut s, "A completely different title");
        assert_eq!(s, "A completely different title");
        e.move_to(s.len(), false);
        e.anchor = previous(&s, e.cursor);
        e.insert(&mut s, "");
        assert_eq!(s, "A completely different titl");
        assert_eq!(following("é", 0), 2);
        assert_eq!(previous("é", 2), 0);
    }
}
