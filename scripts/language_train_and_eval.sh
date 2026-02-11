#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/.."
INPUT="${1:-sharegpt_pairs.zh.simplified_native.jsonl}"
mkdir -p data
cargo run --bin clean_language_data -- "$INPUT" data/cleaned_language.jsonl --split=0.1
cargo run --bin logos -- --import data/cleaned_language_train.jsonl --eval data/cleaned_language_test.jsonl
