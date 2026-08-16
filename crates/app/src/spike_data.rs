use std::time::Duration;

pub const STREAM_INTERVAL: Duration = Duration::from_millis(30);
pub const TOKENS_PER_CHUNK: usize = 80;
pub const MIN_STREAM_TOKENS: usize = 8_000;
const TARGET_SECTIONS: usize = 100;

pub struct StreamDocument {
    pub markdown: String,
    pub chunks: Vec<String>,
    pub token_count: usize,
}

pub fn generate_stream_document() -> StreamDocument {
    let mut markdown = String::with_capacity(96_000);
    for section in 1..=TARGET_SECTIONS {
        markdown.push_str(&format!(
            "## Section {section:03}: streaming benchmark\n\n\
             This structured paragraph measures incremental markdown rendering with stable whitespace tokens. \
             Each section repeats predictable prose so token accounting and chunk coverage remain deterministic.\n\n\
             - item alpha keeps list parsing active during the stream\n\
             - item beta includes **bold text**, *emphasis*, and `inline_code_{section:03}`\n\
             - item gamma records section_{section:03} progress for the benchmark HUD\n\n\
             ```rust\n\
             fn section_{section:03}() -> usize {{\n\
                 let frame_budget_ms = 20;\n\
                 frame_budget_ms + {section}\n\
             }}\n\
             ```\n\n\
             > The stream cadence is fixed at thirty milliseconds per chunk and the target floor is eight thousand tokens.\n\n"
        ));
    }

    let token_count = markdown.split_whitespace().count();
    let chunks = chunk_at_whitespace_boundaries(&markdown, TOKENS_PER_CHUNK);

    StreamDocument {
        markdown,
        chunks,
        token_count,
    }
}

fn chunk_at_whitespace_boundaries(markdown: &str, tokens_per_chunk: usize) -> Vec<String> {
    assert!(tokens_per_chunk > 0);

    let mut chunks = Vec::new();
    let mut chunk_start = 0;
    let mut token_count = 0;
    let mut in_token = false;

    for (index, character) in markdown.char_indices() {
        if character.is_whitespace() {
            if in_token {
                token_count += 1;
                in_token = false;
            }
            if token_count >= tokens_per_chunk {
                let end = index + character.len_utf8();
                chunks.push(markdown[chunk_start..end].to_owned());
                chunk_start = end;
                token_count = 0;
            }
        } else {
            in_token = true;
        }
    }

    if chunk_start < markdown.len() {
        chunks.push(markdown[chunk_start..].to_owned());
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_document_meets_token_floor() {
        let document = generate_stream_document();
        assert!(document.token_count >= MIN_STREAM_TOKENS);
        assert_eq!(
            document.token_count,
            document.markdown.split_whitespace().count()
        );
    }

    #[test]
    fn chunks_are_non_empty_and_cover_document_exactly() {
        let document = generate_stream_document();
        assert!(!document.chunks.is_empty());
        assert!(document.chunks.iter().all(|chunk| !chunk.is_empty()));
        assert_eq!(document.chunks.concat(), document.markdown);
    }

    #[test]
    fn stream_contract_constants_do_not_drift() {
        assert_eq!(STREAM_INTERVAL, Duration::from_millis(30));
        assert_eq!(TOKENS_PER_CHUNK, 80);
        assert_eq!(TARGET_SECTIONS, 100);
    }
}
