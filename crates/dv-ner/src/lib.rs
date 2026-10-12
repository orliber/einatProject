//! Local name detection with a Hebrew NER model (plan stage 5, D-053: optional, off by default).
//!
//! The model (DictaBERT-NER by default: a BERT token classifier) is read from a folder on
//! disk with `config.json`, `tokenizer.json` and `model.safetensors`, and runs on the CPU in
//! pure Rust (`candle`). This crate has no network code (`cargo xtask check-invariants`); the
//! files are downloaded by `dv-egress` only when she turns the model on. Its spans are hints
//! only: `dv-privacy` may add a hide for them, never remove one a rule made.

pub mod worker;

use std::fmt;
use std::path::Path;

use candle_core::{DType, Device, Tensor, D};
use candle_nn::{Linear, Module, VarBuilder};
use candle_transformers::models::bert::{BertModel, Config};
use tokenizers::Tokenizer;

/// Pieces per window, besides the `[CLS]` and `[SEP]` the model adds.
const WINDOW: usize = 510;
/// Pieces on each side of a window that only give context; their labels come from the
/// next window, where they sit in the middle.
const CONTEXT: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum NerError {
    #[error("model files: {0}")]
    Files(String),
    #[error("model: {0}")]
    Model(#[from] candle_core::Error),
    #[error("tokenizer: {0}")]
    Tokenizer(String),
}

/// One entity the model found, as a byte range of the text it was given.
#[derive(Debug, Clone, PartialEq)]
pub struct NerSpan {
    pub start: usize,
    pub end: usize,
    /// The entity type without the B-/I- prefix ("PER", "GPE", "ORG").
    pub label: String,
    /// Mean probability of the label over the span's pieces.
    pub score: f32,
}

pub struct NerModel {
    bert: BertModel,
    classifier: Linear,
    tokenizer: Tokenizer,
    /// id → label, from `config.json` ("O", "B-PER", "I-PER", ...).
    labels: Vec<String>,
    cls: u32,
    sep: u32,
    device: Device,
}

impl fmt::Debug for NerModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NerModel")
            .field("labels", &self.labels)
            .finish_non_exhaustive()
    }
}

/// `id2label` from a Hugging Face `config.json`, in id order.
fn labels_of(config: &serde_json::Value) -> Result<Vec<String>, NerError> {
    let map = config
        .get("id2label")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| NerError::Files("config.json has no id2label".to_owned()))?;
    let mut labels = vec![String::new(); map.len()];
    for (id, label) in map {
        let i: usize = id
            .parse()
            .map_err(|_| NerError::Files(format!("bad label id {id}")))?;
        let slot = labels
            .get_mut(i)
            .ok_or_else(|| NerError::Files(format!("label id {i} out of range")))?;
        *slot = label.as_str().unwrap_or_default().to_owned();
    }
    if labels.iter().any(String::is_empty) {
        return Err(NerError::Files("id2label has gaps".to_owned()));
    }
    Ok(labels)
}

impl NerModel {
    /// Load the model from `dir` (`config.json`, `tokenizer.json`, `model.safetensors`).
    pub fn load(dir: &Path) -> Result<Self, NerError> {
        let read = |name: &str| {
            std::fs::read(dir.join(name)).map_err(|e| NerError::Files(format!("{name}: {e}")))
        };
        let raw: serde_json::Value = serde_json::from_slice(&read("config.json")?)
            .map_err(|e| NerError::Files(format!("config.json: {e}")))?;
        let labels = labels_of(&raw)?;
        let config: Config = serde_json::from_value(raw)
            .map_err(|e| NerError::Files(format!("config.json: {e}")))?;
        let tokenizer = Tokenizer::from_bytes(read("tokenizer.json")?)
            .map_err(|e| NerError::Tokenizer(e.to_string()))?;
        let id = |t: &str| {
            tokenizer
                .token_to_id(t)
                .ok_or_else(|| NerError::Tokenizer(format!("no {t} in the vocabulary")))
        };
        let (cls, sep) = (id("[CLS]")?, id("[SEP]")?);
        let device = Device::Cpu;
        let weights = read("model.safetensors")?;
        let vb = VarBuilder::from_buffered_safetensors(weights, DType::F32, &device)?;
        let bert = BertModel::load(vb.clone(), &config)?;
        let classifier = candle_nn::linear(config.hidden_size, labels.len(), vb.pp("classifier"))?;
        Ok(Self {
            bert,
            classifier,
            tokenizer,
            labels,
            cls,
            sep,
            device,
        })
    }

    /// The best label of each piece, with its probability, for one window of piece ids.
    fn classify(&self, ids: &[u32]) -> Result<Vec<(usize, f32)>, NerError> {
        let mut input = Vec::with_capacity(ids.len() + 2);
        input.push(self.cls);
        input.extend_from_slice(ids);
        input.push(self.sep);
        let input = Tensor::new(input.as_slice(), &self.device)?.unsqueeze(0)?;
        let types = input.zeros_like()?;
        let hidden = self.bert.forward(&input, &types, None)?;
        let probs = candle_nn::ops::softmax(&self.classifier.forward(&hidden)?, D::Minus1)?;
        let probs: Vec<Vec<f32>> = probs.squeeze(0)?.to_vec2()?;
        // Drop [CLS] and [SEP].
        let inner = probs.get(1..probs.len().saturating_sub(1)).unwrap_or(&[]);
        Ok(inner
            .iter()
            .map(|row| {
                row.iter()
                    .copied()
                    .enumerate()
                    .fold(
                        (0, f32::MIN),
                        |best, (i, p)| if p > best.1 { (i, p) } else { best },
                    )
            })
            .collect())
    }

    /// Every entity in `text`, in order. Long texts are read in overlapping windows.
    pub fn entities(&self, text: &str) -> Result<Vec<NerSpan>, NerError> {
        let enc = self
            .tokenizer
            .encode(text, false)
            .map_err(|e| NerError::Tokenizer(e.to_string()))?;
        let ids = enc.get_ids();
        let offsets = enc.get_offsets();
        let mut best: Vec<(usize, f32)> = Vec::with_capacity(ids.len());
        let mut start = 0;
        while best.len() < ids.len() {
            let end = (start + WINDOW).min(ids.len());
            let window = self.classify(ids.get(start..end).unwrap_or(&[]))?;
            let keep_to = if end < ids.len() { end - CONTEXT } else { end };
            let from = best.len() - start;
            best.extend(window.get(from..keep_to - start).unwrap_or(&[]));
            start = best.len().saturating_sub(CONTEXT);
        }
        Ok(merge(&best, offsets, &self.labels))
    }

    /// People only: the spans the filter takes as names.
    pub fn people(&self, text: &str) -> Result<Vec<NerSpan>, NerError> {
        let mut all = self.entities(text)?;
        all.retain(|s| s.label == "PER");
        Ok(all)
    }
}

/// Pieces with the same entity type, next to each other, are one entity (a B- piece starts a
/// new one). Pieces the tokenizer gives no text to (offsets 0..0) are skipped.
fn merge(best: &[(usize, f32)], offsets: &[(usize, usize)], labels: &[String]) -> Vec<NerSpan> {
    let mut out: Vec<NerSpan> = Vec::new();
    let mut count = 0_u32;
    let mut open = false;
    for (&(label, p), &(s, e)) in best.iter().zip(offsets) {
        if s == e {
            continue;
        }
        let tag = labels.get(label).map_or("O", String::as_str);
        let (begin, kind) = match tag.split_once('-') {
            Some(("B", k)) => (true, k),
            Some((_, k)) => (false, k),
            None => {
                open = false;
                continue;
            }
        };
        match out.last_mut() {
            Some(last) if open && !begin && last.label == kind => {
                last.end = e;
                last.score = (last.score * count as f32 + p) / (count + 1) as f32;
                count += 1;
            }
            _ => {
                out.push(NerSpan {
                    start: s,
                    end: e,
                    label: kind.to_owned(),
                    score: p,
                });
                count = 1;
                open = true;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn labels() -> Vec<String> {
        ["O", "B-PER", "I-PER", "B-GPE", "I-GPE"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect()
    }

    #[test]
    fn pieces_merge_into_entities() {
        // "הגננת אס ##תי מ ##חיפה"
        let best = [(0, 0.9), (1, 0.8), (2, 0.6), (0, 0.9), (4, 0.7)];
        let offsets = [(0, 10), (11, 15), (15, 19), (20, 22), (22, 30)];
        let got = merge(&best, &offsets, &labels());
        assert_eq!(got.len(), 2);
        assert_eq!(
            (got[0].start, got[0].end, got[0].label.as_str()),
            (11, 19, "PER")
        );
        assert!((got[0].score - 0.7).abs() < 1e-6);
        // An I- piece with no open entity still opens one (fail-closed).
        assert_eq!(
            (got[1].start, got[1].end, got[1].label.as_str()),
            (22, 30, "GPE")
        );
    }

    #[test]
    fn a_b_piece_starts_a_new_entity() {
        let best = [(1, 0.9), (1, 0.9)];
        let offsets = [(0, 4), (5, 9)];
        assert_eq!(merge(&best, &offsets, &labels()).len(), 2);
    }

    #[test]
    fn labels_come_in_id_order() {
        let config = serde_json::json!({"id2label": {"1": "B-PER", "0": "O"}});
        assert_eq!(labels_of(&config).unwrap(), vec!["O", "B-PER"]);
        let gaps = serde_json::json!({"id2label": {"0": "O", "2": "B-PER"}});
        assert!(labels_of(&gaps).is_err());
    }
}
