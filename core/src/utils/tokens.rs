use std::sync::OnceLock;
use tiktoken_rs::CoreBPE;

static TOKENIZER: OnceLock<CoreBPE> = OnceLock::new();

fn get_tokenizer() -> &'static CoreBPE {
    TOKENIZER.get_or_init(|| {
        tiktoken_rs::cl100k_base().expect("failed to load cl100k_base tokenizer")
    })
}

/// Count tokens accurately using cl100k_base (Claude's tokenizer family).
pub fn count_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    get_tokenizer().encode_ordinary(text).len()
}

/// Fast approximate token count (~3.5 chars per token).
/// Use this in hot paths where accuracy isn't critical.
pub fn estimate_tokens_fast(text: &str) -> usize {
    (text.len() as f64 / 3.5).ceil() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_string_is_zero_tokens() {
        assert_eq!(count_tokens(""), 0);
    }

    #[test]
    fn basic_counting() {
        let tokens = count_tokens("Hello, world!");
        assert!(tokens > 0 && tokens < 10);
    }

    #[test]
    fn fast_estimate_is_close() {
        let text = "This is a test string with some words in it for estimation.";
        let accurate = count_tokens(text);
        let fast = estimate_tokens_fast(text);
        // Should be within 2x of each other
        assert!(fast > accurate / 2 && fast < accurate * 2);
    }
}
