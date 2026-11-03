//! Independent scalar inference on explicitly supplied plain model graphs.
#![deny(unsafe_code)]

pub mod acoustic;
pub mod activation;
pub mod compare;
pub mod dnni;
pub mod frontend;
pub mod graph;
pub mod japanese;
pub mod language;
pub mod lingual;
pub mod manifest;
mod memory;
pub mod modern;
pub mod normalization;
pub mod phonemes;
pub mod pqmf;
pub mod prosody;
pub mod sampling;
pub mod scalar_layers;
pub mod sequence;
pub mod stylebook;
pub mod synthesis;
pub mod tensor;
pub mod vocoder;
pub mod wave;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self(e.to_string())
    }
}

pub(crate) fn invalid(message: impl Into<String>) -> Error {
    Error(message.into())
}
