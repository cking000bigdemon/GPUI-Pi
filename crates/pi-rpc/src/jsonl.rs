//! 严格的 LF-only JSONL 分帧。

use thiserror::Error;

/// RPC 单帧默认上限。足以容纳大型工具结果，同时避免损坏的子进程无限占用内存。
pub const DEFAULT_MAX_FRAME_LEN: usize = 16 * 1024 * 1024;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum JsonlError {
    #[error("JSONL frame exceeds {max} bytes")]
    FrameTooLarge { max: usize },
}

/// 增量 JSONL 分帧器。只把字节 `LF` 当作记录分隔符。
#[derive(Debug)]
pub struct JsonlFramer {
    buffer: Vec<u8>,
    max_frame_len: usize,
}

impl Default for JsonlFramer {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_FRAME_LEN)
    }
}

impl JsonlFramer {
    pub fn new(max_frame_len: usize) -> Self {
        assert!(max_frame_len > 0, "max_frame_len must be positive");
        Self {
            buffer: Vec::new(),
            max_frame_len,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>, JsonlError> {
        self.buffer.extend_from_slice(bytes);
        let mut frames = Vec::new();

        while let Some(newline) = self.buffer.iter().position(|byte| *byte == b'\n') {
            if newline > self.max_frame_len {
                self.buffer.clear();
                return Err(JsonlError::FrameTooLarge {
                    max: self.max_frame_len,
                });
            }
            let mut frame: Vec<u8> = self.buffer.drain(..=newline).collect();
            frame.pop();
            strip_trailing_cr(&mut frame);
            frames.push(frame);
        }

        if self.buffer.len() > self.max_frame_len {
            self.buffer.clear();
            return Err(JsonlError::FrameTooLarge {
                max: self.max_frame_len,
            });
        }
        Ok(frames)
    }

    /// 流结束时发出最后一个非空且没有尾 LF 的记录。
    pub fn finish(&mut self) -> Result<Option<Vec<u8>>, JsonlError> {
        if self.buffer.len() > self.max_frame_len {
            self.buffer.clear();
            return Err(JsonlError::FrameTooLarge {
                max: self.max_frame_len,
            });
        }
        if self.buffer.is_empty() {
            return Ok(None);
        }
        let mut frame = std::mem::take(&mut self.buffer);
        strip_trailing_cr(&mut frame);
        Ok(Some(frame))
    }
}

fn strip_trailing_cr(frame: &mut Vec<u8>) {
    if frame.last() == Some(&b'\r') {
        frame.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_only_on_lf_across_chunks() {
        let mut framer = JsonlFramer::new(128);
        assert!(framer.push(b"{\"x\":\"").unwrap().is_empty());
        let frames = framer
            .push("a\u{2028}b\u{2029}c\"}\r\nnext\n".as_bytes())
            .unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(
            std::str::from_utf8(&frames[0]).unwrap(),
            "{\"x\":\"a\u{2028}b\u{2029}c\"}"
        );
        assert_eq!(frames[1], b"next");
    }

    #[test]
    fn emits_non_empty_tail_at_eof() {
        let mut framer = JsonlFramer::new(16);
        framer.push(b"tail\r").unwrap();
        assert_eq!(framer.finish().unwrap(), Some(b"tail".to_vec()));
        assert_eq!(framer.finish().unwrap(), None);
    }

    #[test]
    fn rejects_oversized_complete_and_partial_frames() {
        let mut partial = JsonlFramer::new(3);
        assert_eq!(
            partial.push(b"1234").unwrap_err(),
            JsonlError::FrameTooLarge { max: 3 }
        );

        let mut complete = JsonlFramer::new(3);
        assert_eq!(
            complete.push(b"1234\n").unwrap_err(),
            JsonlError::FrameTooLarge { max: 3 }
        );
    }
}
