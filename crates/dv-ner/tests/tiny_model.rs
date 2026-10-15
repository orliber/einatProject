//! The whole path (files on disk → windows → spans) on a tiny model written by the test.
//! Its classifier always answers B-PER, so every word must come back as its own span with
//! the right byte range, also past the first window.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::fmt::Write as _;
use std::path::PathBuf;

use candle_core::{DType, Device, Tensor};
use candle_nn::{VarBuilder, VarMap};
use candle_transformers::models::bert::{BertModel, Config};
use dv_ner::NerModel;

const WORDS: &[&str] = &["שלום", "הגננת", "אסתי", "סיפרה", "ש", "נוגה", "שיחקה"];

fn write_model() -> PathBuf {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(write_model_once).clone()
}

fn write_model_once() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("dv-ner-tiny");
    std::fs::create_dir_all(&dir).unwrap();
    let config = serde_json::json!({
        "vocab_size": WORDS.len() + 3, "hidden_size": 8, "num_hidden_layers": 1,
        "num_attention_heads": 2, "intermediate_size": 16, "hidden_act": "gelu",
        "hidden_dropout_prob": 0.0, "max_position_embeddings": 600, "type_vocab_size": 2,
        "initializer_range": 0.02, "layer_norm_eps": 1e-12, "pad_token_id": 0,
        "model_type": "bert",
        "id2label": {"0": "O", "1": "B-PER", "2": "I-PER"}
    });
    std::fs::write(dir.join("config.json"), config.to_string()).unwrap();
    let mut vocab = serde_json::Map::new();
    for (i, w) in ["[UNK]", "[CLS]", "[SEP]"].iter().chain(WORDS).enumerate() {
        vocab.insert((*w).to_owned(), i.into());
    }
    let tokenizer = serde_json::json!({
        "version": "1.0", "truncation": null, "padding": null, "added_tokens": [],
        "normalizer": null, "pre_tokenizer": {"type": "Whitespace"},
        "post_processor": null, "decoder": null,
        "model": {"type": "WordLevel", "vocab": vocab, "unk_token": "[UNK]"}
    });
    std::fs::write(dir.join("tokenizer.json"), tokenizer.to_string()).unwrap();

    let cfg: Config = serde_json::from_value(config).unwrap();
    let mut vm = VarMap::new();
    let vb = VarBuilder::from_varmap(&vm, DType::F32, &Device::Cpu);
    BertModel::load(vb.pp("bert"), &cfg).unwrap();
    candle_nn::linear(cfg.hidden_size, 3, vb.pp("classifier")).unwrap();
    vm.set_one(
        "classifier.weight",
        Tensor::zeros((3, cfg.hidden_size), DType::F32, &Device::Cpu).unwrap(),
    )
    .unwrap();
    vm.set_one(
        "classifier.bias",
        Tensor::new(&[0.0_f32, 10.0, 0.0], &Device::Cpu).unwrap(),
    )
    .unwrap();
    vm.save(dir.join("model.safetensors")).unwrap();
    dir
}

#[test]
fn every_word_comes_back_with_its_byte_range_across_windows() {
    let model = NerModel::load(&write_model()).unwrap();
    let mut text = String::new();
    let mut want = Vec::new();
    for i in 0..1300 {
        let w = WORDS[i % WORDS.len()];
        if !text.is_empty() {
            text.push(if i % 11 == 0 { '\n' } else { ' ' });
        }
        want.push((text.len(), text.len() + w.len()));
        write!(text, "{w}").unwrap();
    }
    let got = model.people(&text).unwrap();
    let got: Vec<(usize, usize)> = got.iter().map(|s| (s.start, s.end)).collect();
    assert_eq!(got, want);
}

#[test]
fn missing_files_are_an_error_not_a_panic() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("dv-ner-none");
    assert!(NerModel::load(&dir).is_err());
}

fn worker_exe() -> &'static std::path::Path {
    std::path::Path::new(env!("CARGO_BIN_EXE_dv-ner-worker"))
}

/// SEC2: the same answers from the isolated worker, many texts on one loaded model.
#[test]
fn the_worker_answers_like_the_model_and_keeps_running_between_texts() {
    use dv_ner::worker::NerWorker;
    let dir = write_model();
    let model = NerModel::load(&dir).unwrap();
    let mut worker = NerWorker::start(worker_exe(), &dir).unwrap();
    for text in [
        "שלום נוגה",
        "הגננת אסתי סיפרה ש נוגה שיחקה",
        "",
        "שלום\nשלום",
    ] {
        let want: Vec<(usize, usize)> = model
            .people(text)
            .unwrap()
            .iter()
            .map(|s| (s.start, s.end))
            .collect();
        let got: Vec<(usize, usize)> = worker
            .people(text)
            .unwrap()
            .iter()
            .map(|p| (p.start, p.end))
            .collect();
        assert_eq!(got, want, "{text}");
    }
}

#[test]
fn a_worker_without_model_files_does_not_start() {
    use dv_ner::worker::{NerWorker, WorkerError};
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("dv-ner-none");
    assert!(matches!(
        NerWorker::start(worker_exe(), &dir),
        Err(WorkerError::Load(_))
    ));
}
