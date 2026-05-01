// Item linked-list and prefix/exact/substring matching, ported from dmenu.c.

use crate::state::Menu;

#[derive(Default)]
pub struct Item {
    pub text: Vec<u8>,
    pub left: Option<usize>,
    pub right: Option<usize>,
}

fn cmp_eq(a: &[u8], b: &[u8], ci: bool) -> bool {
    if a.len() != b.len() { return false; }
    if ci {
        a.iter().zip(b).all(|(x, y)| x.to_ascii_lowercase() == y.to_ascii_lowercase())
    } else {
        a == b
    }
}

fn starts_with(haystack: &[u8], needle: &[u8], ci: bool) -> bool {
    if haystack.len() < needle.len() { return false; }
    if ci {
        haystack.iter().zip(needle).all(|(x, y)| x.to_ascii_lowercase() == y.to_ascii_lowercase())
    } else {
        &haystack[..needle.len()] == needle
    }
}

fn contains_sub(haystack: &[u8], needle: &[u8], ci: bool) -> bool {
    if needle.is_empty() { return true; }
    if needle.len() > haystack.len() { return false; }
    for i in 0..=haystack.len() - needle.len() {
        if starts_with(&haystack[i..], needle, ci) {
            return true;
        }
    }
    false
}

fn appenditem(items: &mut [Item], idx: usize,
    list: &mut Option<usize>, last: &mut Option<usize>)
{
    if last.is_none() {
        *list = Some(idx);
    } else if let Some(li) = *last {
        items[li].right = Some(idx);
    }
    items[idx].left = *last;
    items[idx].right = None;
    *last = Some(idx);
}

pub fn nextrune(text: &[u8], cursor: usize, inc: i32) -> isize {
    let mut n = cursor as isize + inc as isize;
    while n + inc as isize >= 0
        && (n as usize) < text.len()
        && (text[n as usize] & 0xc0) == 0x80
    {
        n += inc as isize;
    }
    n
}

impl Menu {
    pub fn do_match(&mut self, sub: bool) {
        let mut lexact: Option<usize> = None;
        let mut lprefix: Option<usize> = None;
        let mut lsubstr: Option<usize> = None;
        let mut exactend: Option<usize> = None;
        let mut prefixend: Option<usize> = None;
        let mut substrend: Option<usize> = None;

        let order: Vec<usize> = if sub {
            let mut v = Vec::new();
            let mut cur = self.matches;
            while let Some(i) = cur {
                v.push(i);
                cur = self.items[i].right;
            }
            v
        } else {
            (0..self.items.len()).collect()
        };

        let ci = self.args.case_insensitive;
        let needle: Vec<u8> = self.text.clone();
        for idx in order {
            let txt = self.items[idx].text.clone();
            if cmp_eq(&txt, &needle, ci) {
                appenditem(&mut self.items, idx, &mut lexact, &mut exactend);
            } else if starts_with(&txt, &needle, ci) {
                appenditem(&mut self.items, idx, &mut lprefix, &mut prefixend);
            } else if contains_sub(&txt, &needle, ci) {
                appenditem(&mut self.items, idx, &mut lsubstr, &mut substrend);
            }
        }

        self.matches = lexact;
        self.matchend = exactend;
        if let Some(p) = lprefix {
            if let Some(me) = self.matchend {
                self.items[me].right = Some(p);
                self.items[p].left = Some(me);
            } else {
                self.matches = lprefix;
            }
            self.matchend = prefixend;
        }
        if let Some(s) = lsubstr {
            if let Some(me) = self.matchend {
                self.items[me].right = Some(s);
                self.items[s].left = Some(me);
            } else {
                self.matches = lsubstr;
            }
            self.matchend = substrend;
        }
        self.curr = self.matches;
        self.sel = self.matches;
        self.calcoffsets();
    }

    pub fn calcoffsets(&mut self) {
        let n = if self.args.lines > 0 {
            self.args.lines * self.bh
        } else {
            let lt = self.text_w(b"<");
            let gt = self.text_w(b">");
            self.mw - (self.promptw + self.inputw + lt + gt)
        };
        let mut i: i32 = 0;
        let mut next = self.curr;
        while let Some(idx) = next {
            let inc = if self.args.lines > 0 { self.bh } else {
                self.text_w(&self.items[idx].text.clone()).min(n)
            };
            i += inc;
            if i > n { break; }
            next = self.items[idx].right;
        }
        self.next = next;

        let mut i: i32 = 0;
        let mut prev = self.curr;
        while let Some(idx) = prev {
            if let Some(li) = self.items[idx].left {
                let inc = if self.args.lines > 0 { self.bh } else {
                    self.text_w(&self.items[li].text.clone()).min(n)
                };
                i += inc;
                if i > n { break; }
                prev = Some(li);
            } else {
                break;
            }
        }
        self.prev = prev;
    }
}
