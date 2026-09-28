// Keep escape-parser state across Docker stream chunks. Logs must not execute
// terminal controls or display the payloads of OSC title/clipboard sequences.
#[derive(Default)]
pub(super) struct LogText {
    state: u8,
}
impl LogText {
    pub(super) fn feed(&mut self, input: &str) -> String {
        let mut output = String::new();
        for ch in input.chars() {
            match self.state {
                0 => match ch {
                    '\x1b' => self.state = 1,
                    '\n' | '\t' => output.push(ch),
                    c if !c.is_control() => output.push(c),
                    _ => {}
                },
                1 => {
                    self.state = match ch {
                        '[' => 2,
                        ']' | 'P' | 'X' | '^' | '_' => 3,
                        '\x20'..='\x2f' => 5,
                        _ => 0,
                    }
                }
                2 => {
                    if ('\x40'..='\x7e').contains(&ch) {
                        self.state = 0;
                    }
                }
                3 => match ch {
                    '\x07' => self.state = 0,
                    '\x1b' => self.state = 4,
                    _ => {}
                },
                4 => self.state = if ch == '\\' { 0 } else { 3 },
                5 => {
                    if ('\x30'..='\x7e').contains(&ch) {
                        self.state = 0;
                    }
                }
                _ => self.state = 0,
            }
        }
        output
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removes_split_color_and_osc_sequences() {
        let mut text = LogText::default();
        assert_eq!(text.feed("a\x1b[3"), "a");
        assert_eq!(text.feed("1mred\x1b[0m\n\x1b]52;secret"), "red\n");
        assert_eq!(text.feed("\x07b"), "b");
    }
}
