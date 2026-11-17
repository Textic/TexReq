pub mod curl_parser;
pub mod http_client;
pub mod interpolator;
pub mod runtime;
pub mod snippets;
pub mod ws_client;

use crate::models::{HttpResponseData, StreamEntry, StreamStatus};

#[derive(Debug, Clone)]
pub enum WorkerEvent {
    HttpFinished(Result<HttpResponseData, String>),
    StreamStatus(StreamStatus),
    StreamMessage(StreamEntry),
    StreamError(String),
}
