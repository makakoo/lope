//! `_BoundedStream` (`supervisor.py:39-63`): complete output up to the cap,
//! then a bounded head + marker + tail.

pub const TRUNCATION_MARKER: &[u8] = b"\n...[output truncated; head and tail preserved]...\n";

#[derive(Debug, Clone)]
pub struct BoundedStream {
    pub limit: usize,
    pub total: usize,
    data: Vec<u8>,
}

impl BoundedStream {
    pub fn new(limit: usize) -> Self {
        BoundedStream { limit, total: 0, data: Vec::new() }
    }

    /// Returns true once the limit has been exceeded.
    pub fn add(&mut self, chunk: &[u8]) -> bool {
        self.total += chunk.len();
        if self.total <= self.limit {
            self.data.extend_from_slice(chunk);
            return false;
        }
        let mut combined = std::mem::take(&mut self.data);
        combined.extend_from_slice(chunk);
        let marker = &TRUNCATION_MARKER[..TRUNCATION_MARKER.len().min(self.limit)];
        let remaining = self.limit.saturating_sub(marker.len());
        let head = remaining / 2;
        let tail = remaining - head;
        let mut out = Vec::with_capacity(self.limit);
        out.extend_from_slice(&combined[..head.min(combined.len())]);
        out.extend_from_slice(marker);
        if tail > 0 {
            // Python: combined[-tail_size:] (whole buffer if shorter)
            let start = combined.len().saturating_sub(tail);
            out.extend_from_slice(&combined[start..]);
        }
        self.data = out;
        true
    }

    pub fn value(&self) -> &[u8] {
        &self.data[..self.data.len().min(self.limit)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_and_tail() {
        let mut b = BoundedStream::new(80);
        assert!(!b.add(&[b'a'; 50]));
        assert!(b.add(&[b'z'; 50]));
        let v = b.value();
        assert_eq!(v.len(), 80);
        assert!(v.starts_with(b"aaaaaaaaaaaaaa"));
        assert!(v.ends_with(b"zzzzzzzzzzzzzz"));
        let mut tiny = BoundedStream::new(10);
        tiny.add(b"0123456789AB");
        assert_eq!(tiny.value(), &TRUNCATION_MARKER[..10]);
    }
}
