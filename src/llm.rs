use serde::{Deserialize, Serialize};
pub const MODEL: &'static str = "gemma3";
#[derive(Serialize)]
pub(crate) struct LLMRequest {
    pub(crate) model: String,
    pub(crate) stream: bool,
    pub(crate) messages: Vec<Message>,
    pub(crate) options: LLMOptions,
}
#[derive(Serialize)]
pub(crate) struct Message {
    pub(crate) role: String,
    pub(crate) content: String,
}

#[derive(Serialize)]
pub(crate) struct LLMOptions {
    pub(crate) temperature: f32,
    pub(crate) response_format: String,
}

#[derive(Deserialize)]
pub(crate) struct LLMResponse {
    pub(crate) message: LLMMessageContent,
}

#[derive(Deserialize)]
pub(crate) struct LLMMessageContent {
    pub(crate) content: String,
}
