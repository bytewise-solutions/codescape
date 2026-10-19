pub struct WindowStack<W: Copy + PartialEq> {
    stack: Vec<W>,
}

impl<W: Copy + PartialEq> Default for WindowStack<W> {
    fn default() -> Self {
        Self { stack: Vec::new() }
    }
}

impl<W: Copy + PartialEq> WindowStack<W> {
    pub fn sync(&mut self, open: &[W], top: Option<W>) {
        self.stack.retain(|w| open.contains(w));
        for &w in open {
            if !self.stack.contains(&w) {
                self.stack.push(w);
            }
        }
        if let Some(i) = top.and_then(|t| self.stack.iter().position(|&w| w == t)) {
            let w = self.stack.remove(i);
            self.stack.push(w);
        }
    }

    pub fn pop(&mut self) -> Option<W> {
        self.stack.pop()
    }
}
