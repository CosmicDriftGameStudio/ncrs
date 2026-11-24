//! Picks the reply tokens: the likeliest token the grammar allows.
//!
//! Handing the grammar the whole vocabulary for every token costs far more
//! than the model step itself. Like llama.cpp's `common_sampler`, the likeliest
//! token is taken first and only checked against the grammar; the full
//! vocabulary goes through the grammar only when that token is refused.

use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::data::LlamaTokenData;
use llama_cpp_2::token::data_array::LlamaTokenDataArray;
use llama_cpp_2::token::LlamaToken;

pub(super) struct ReplySampler {
    grammar: LlamaSampler,
}

impl ReplySampler {
    pub(super) fn new(grammar: LlamaSampler) -> Self {
        Self { grammar }
    }

    /// The token for the logits at `index` of the last decode; the grammar has
    /// taken it in when this returns.
    pub(super) fn next(&mut self, context: &LlamaContext<'_>, index: i32) -> LlamaToken {
        let likeliest = context.candidates_ith(index).reduce(|best, candidate| {
            if candidate.logit() > best.logit() {
                candidate
            } else {
                best
            }
        });
        let token = match likeliest {
            Some(candidate) if self.allows(candidate) => candidate.id(),
            _ => {
                let mut all = context.token_data_array_ith(index);
                self.grammar.apply(&mut all);
                all.sample_token_greedy()
            }
        };
        self.grammar.accept(token);
        token
    }

    /// The grammar masks a token it does not allow with minus infinity.
    fn allows(&mut self, candidate: LlamaTokenData) -> bool {
        let mut one = LlamaTokenDataArray::new(vec![candidate], false);
        self.grammar.apply(&mut one);
        one.data
            .first()
            .is_some_and(|checked| checked.logit() > f32::NEG_INFINITY)
    }
}
